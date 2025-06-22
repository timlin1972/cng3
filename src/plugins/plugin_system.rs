use async_trait::async_trait;
use log::Level::Info;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::{
    select,
    time::{Duration, sleep},
};

use crate::messages::{ACTION_ONBOARD, ACTION_PUBLISH, ACTION_SHOW, Cmd, Data, Log, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils;

const MODULE: &str = "system";
const VERSION: &str = "3.0.0";
const PUBLISH_INTERVAL: u64 = 300;

#[derive(Debug)]
struct SystemInfo {
    version: String,
    tailscale_ip: Option<String>,
}

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    system_info: SystemInfo,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");

        let msg_tx_clone = msg_tx.clone();
        let mut shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(async move {
            loop {
                select! {
                    _ = sleep(Duration::from_secs(PUBLISH_INTERVAL)) => {
                        let msg = Msg {
                            ts: utils::ts(),
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
                tailscale_ip: utils::get_tailscale_ip(),
            },
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.log(
            MODULE,
            Info,
            format!("[{MODULE}] Version: v{}", self.system_info.version),
        )
        .await;
        self.log(
            MODULE,
            Info,
            format!(
                "[{MODULE}] Tailscale IP: {}",
                self.system_info
                    .tailscale_ip
                    .clone()
                    .unwrap_or("n/a".to_string())
            ),
        )
        .await;
    }

    async fn handle_cmd_publish(&mut self) {
        update_system(&self.msg_tx, &self.system_info).await;
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

async fn update_system(msg_tx: &Sender<Msg>, system_info: &SystemInfo) {
    // onboard
    let msg = Msg {
        ts: utils::ts(),
        module: MODULE.to_string(),
        data: Data::Cmd(Cmd {
            cmd: format!("p mqtt {ACTION_PUBLISH} false {ACTION_ONBOARD} '1'"),
        }),
    };
    let _ = msg_tx.send(msg).await;

    // version
    let msg = Msg {
        ts: utils::ts(),
        module: MODULE.to_string(),
        data: Data::Cmd(Cmd {
            cmd: format!(
                "p mqtt {ACTION_PUBLISH} false version '{}'",
                system_info.version
            ),
        }),
    };
    let _ = msg_tx.send(msg).await;

    // tailscale_ip
    let msg = Msg {
        ts: utils::ts(),
        module: MODULE.to_string(),
        data: Data::Cmd(Cmd {
            cmd: format!(
                "p mqtt {ACTION_PUBLISH} false tailscale_ip '{}'",
                system_info
                    .tailscale_ip
                    .clone()
                    .unwrap_or("n/a".to_string())
            ),
        }),
    };
    let _ = msg_tx.send(msg).await;
}
