use async_trait::async_trait;
use log::Level::Info;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::{
    select,
    time::{Duration, sleep},
};

use crate::messages::{
    ACTION_APP_UPTIME, ACTION_ONBOARD, ACTION_PUBLISH, ACTION_SHOW, ACTION_TAILSCALE_IP,
    ACTION_TEMPERATURE, ACTION_VERSION, Cmd, Data, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, dev_info};

const MODULE: &str = "system";
const VERSION: &str = "3.0.6";
const PUBLISH_INTERVAL: u64 = 300;

#[derive(Debug)]
struct SystemInfo {
    version: String,
    tailscale_ip: Option<String>,
    ts_start: u64,
}

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    system_info: SystemInfo,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        let msg_tx_clone = msg_tx.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                select! {
                    _ = sleep(Duration::from_secs(PUBLISH_INTERVAL)) => {
                        let msg = Msg {
                            ts: utils::time::ts(),
                            module: MODULE.to_string(),
                            data: Data::Cmd(Cmd {
                                cmd: format!("p system {ACTION_PUBLISH}"),
                            })
                        };
                        let _ = &msg_tx_clone.send(msg).await;
                    }
                    _ = shutdown_rx.recv() => {
                        println!("Shutdown signal received. Exiting task.");
                        break;
                    }
                }
            }
        });

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            system_info: SystemInfo {
                version: VERSION.to_string(),
                tailscale_ip: utils::system::get_tailscale_ip(),
                ts_start: utils::time::uptime(),
            },
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.info(
            MODULE,
            format!("[{MODULE}] Version: v{}", self.system_info.version),
        )
        .await;

        self.info(
            MODULE,
            format!(
                "[{MODULE}] Tailscale IP: {}",
                self.system_info
                    .tailscale_ip
                    .clone()
                    .unwrap_or("n/a".to_string())
            ),
        )
        .await;

        self.info(
            MODULE,
            format!(
                "[{MODULE}] Temperature: {}",
                dev_info::temperature_str(Some(get_temperature()))
            ),
        )
        .await;

        self.info(
            MODULE,
            format!(
                "[{MODULE}] App uptime: {}",
                utils::time::uptime_str(utils::time::uptime() - self.system_info.ts_start)
            ),
        )
        .await;
    }

    async fn handle_cmd_publish(&mut self) {
        self.update_system().await;
    }

    async fn update_system(&mut self) {
        // onboard
        self.cmd(
            MODULE,
            format!("p mqtt {ACTION_PUBLISH} false {ACTION_ONBOARD} '1'"),
        )
        .await;

        // version
        self.cmd(
            MODULE,
            format!(
                "p mqtt {ACTION_PUBLISH} false {ACTION_VERSION} '{}'",
                self.system_info.version
            ),
        )
        .await;

        // tailscale_ip
        self.cmd(
            MODULE,
            format!(
                "p mqtt {ACTION_PUBLISH} false {ACTION_TAILSCALE_IP} '{}'",
                self.system_info
                    .tailscale_ip
                    .clone()
                    .unwrap_or("n/a".to_string())
            ),
        )
        .await;

        // temperature
        let temperature = get_temperature();
        self.cmd(
            MODULE,
            format!("p mqtt {ACTION_PUBLISH} false {ACTION_TEMPERATURE} '{temperature}'",),
        )
        .await;

        // app uptime
        let uptime = utils::time::uptime() - self.system_info.ts_start;
        self.cmd(
            MODULE,
            format!("p mqtt {ACTION_PUBLISH} false {ACTION_APP_UPTIME} '{uptime}'",),
        )
        .await;
    }
}

#[async_trait]
impl plugins_main::Plugin for PluginUnit {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    async fn send(&self, msg: Msg) {
        let _ = self.msg_tx.send(msg).await;
    }

    async fn handle_cmd(&mut self, msg: &Msg) {
        if let Data::Cmd(cmd) = &msg.data {
            let cmd_parts = shell_words::split(&cmd.cmd).expect("Failed to parse cmd.");
            if let Some(action) = cmd_parts.get(2) {
                match action.as_str() {
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_PUBLISH => self.handle_cmd_publish().await,
                    _ => {
                        self.log(
                            MODULE,
                            Info,
                            format!(
                                "[{MODULE}] Unknown action ({action}) for cmd `{}`.",
                                cmd.cmd
                            ),
                        )
                        .await;
                    }
                }
            } else {
                self.log(
                    MODULE,
                    Info,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}

fn get_temperature() -> f32 {
    let components = sysinfo::Components::new_with_refreshed_list();
    for component in &components {
        if component.label().to_ascii_lowercase().contains("cpu") {
            return component.temperature().unwrap_or(0.0);
        }
    }

    0.0
}
