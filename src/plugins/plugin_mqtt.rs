use async_trait::async_trait;
use log::Level::{Info, Warn};
use rumqttc::{AsyncClient, Event, Incoming, LastWill, MqttOptions, Publish, QoS};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::cfg;
use crate::messages::{
    ACTION_APP_UPTIME, ACTION_ARROW, ACTION_INIT, ACTION_ONBOARD, ACTION_PUBLISH, ACTION_SHOW,
    ACTION_TAILSCALE_IP, ACTION_TEMPERATURE, ACTION_VERSION, Cmd, Data, Log, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, Mode};

const MODULE: &str = "mqtt";
const BROKER: &str = "broker.emqx.io";
const MQTT_KEEP_ALIVE: u64 = 300;
const RESTART_DELAY: u64 = 60;

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    mode: Mode,
    started: bool,
    gui_panel: String,
    client: Option<AsyncClient>,
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

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            mode: Mode::ModeGui,
            started: false,
            gui_panel: String::new(),
            client: None,
        }
    }

    async fn start_mqtt(&mut self, mut shutdown_rx: broadcast::Receiver<()>) {
        // 1. Initialization
        utils::output_push(
            MODULE,
            &self.msg_tx,
            &self.mode,
            &self.gui_panel,
            Info,
            format!("[{MODULE}] 1/5: Initialization"),
        )
        .await;
        let mut mqttoptions = MqttOptions::new(cfg::name(), BROKER, 1883);
        let will = LastWill::new(
            format!("tln/{}/onboard", cfg::name()),
            "0",
            QoS::AtLeastOnce,
            true,
        );
        mqttoptions
            .set_keep_alive(std::time::Duration::from_secs(MQTT_KEEP_ALIVE))
            .set_last_will(will);

        // 2. Establish connection
        utils::output_push(
            MODULE,
            &self.msg_tx,
            &self.mode,
            &self.gui_panel,
            Info,
            format!("[{MODULE}] 2/5: Establish connection"),
        )
        .await;
        let (client, mut connection) = AsyncClient::new(mqttoptions, 10);

        // 3. Subscribe
        utils::output_push(
            MODULE,
            &self.msg_tx,
            &self.mode,
            &self.gui_panel,
            Info,
            format!("[{MODULE}] 3/5: Subscribe"),
        )
        .await;
        client
            .subscribe("tln/#", QoS::AtMostOnce)
            .await
            .expect("Failed to subscribe");

        // 4. Publish
        utils::output_push(
            MODULE,
            &self.msg_tx,
            &self.mode,
            &self.gui_panel,
            Info,
            format!("[{MODULE}] 4/5: Publish"),
        )
        .await;
        client
            .publish(
                format!("tln/{}/onboard", cfg::name()),
                QoS::AtLeastOnce,
                true,
                "1",
            )
            .await
            .expect("Failed to publish");

        // 5. Receive
        let msg_tx_clone = self.msg_tx.clone();
        let gui_panel_clone = self.gui_panel.clone();
        let mode_clone = self.mode.clone();
        let client_clone = client.clone();
        tokio::spawn(async move {
            utils::output_push(
                MODULE,
                &msg_tx_clone,
                &mode_clone,
                &gui_panel_clone,
                Info,
                format!("[{MODULE}] 5/5: Receive"),
            )
            .await;

            let mut shoutdown_flag = false;
            loop {
                tokio::select! {
                    event = connection.poll() => {
                        if process_event(&msg_tx_clone, &mode_clone, &gui_panel_clone, event).await {
                            break;
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        shoutdown_flag = true;
                        break;
                    }
                }
            }

            utils::output_push(
                MODULE,
                &msg_tx_clone,
                &mode_clone,
                &gui_panel_clone,
                Info,
                format!("[{MODULE}] Disconnect"),
            )
            .await;
            client_clone
                .disconnect()
                .await
                .expect("Failed to disconnect");

            if !shoutdown_flag {
                // restart in RESTART_DELAY seconds
                tokio::time::sleep(tokio::time::Duration::from_secs(RESTART_DELAY)).await;

                let action = match mode_clone {
                    Mode::ModeCli => "cli".to_string(),
                    Mode::ModeGui => format!("gui {}", gui_panel_clone),
                };

                let msg = Msg {
                    ts: utils::ts(),
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!("p mqtt restart {action}"),
                    }),
                };
                let _ = msg_tx_clone.send(msg).await;
            }
        });

        self.client = Some(client);

        // 🧪 補充：錯誤處理與重連
        // - 處理連線失敗、broker 掛掉、封包錯誤等情況
        // - 可設定自動重連機制或 exponential backoff
    }

    async fn handle_cmd_init(
        &mut self,
        cmd_parts: &[String],
        cmd: &Cmd,
        shutdown_rx: broadcast::Receiver<()>,
    ) {
        if let Some(mode) = cmd_parts.get(3) {
            match mode.as_str() {
                "gui" => {
                    // avoid re-entry
                    if self.started && self.mode == Mode::ModeGui {
                        self.log(
                            MODULE,
                            Warn,
                            format!("[{MODULE}] Started and GUI mode already. Ignore."),
                        )
                        .await;
                        return;
                    }

                    if let Some(gui_panel) = cmd_parts.get(4) {
                        self.started = true;
                        self.mode = Mode::ModeGui;
                        self.gui_panel = gui_panel.to_string();

                        self.log(
                            MODULE,
                            Info,
                            format!("[{MODULE}] init gui mode (panel: `{gui_panel}`)"),
                        )
                        .await;
                    }
                }
                "cli" => {
                    // avoid re-entry
                    if self.started && self.mode == Mode::ModeCli {
                        self.log(
                            MODULE,
                            Warn,
                            format!("[{MODULE}] Started and CLI mode already. Ignore."),
                        )
                        .await;
                        return;
                    }
                    self.started = true;
                    self.mode = Mode::ModeCli;

                    self.log(MODULE, Info, format!("[{MODULE}] init cli mode"))
                        .await;
                }
                _ => {
                    self.log(
                        MODULE,
                        Warn,
                        format!("[{MODULE}] Unknown mode ({mode}) for cmd `{}`.", cmd.cmd),
                    )
                    .await;
                    return;
                }
            }

            self.start_mqtt(shutdown_rx).await;
        } else {
            self.log(
                MODULE,
                Warn,
                format!("[{MODULE}] Missing mode for cmd `{}`.", cmd.cmd),
            )
            .await;
        }
    }

    async fn publish(&mut self, topic: &str, retain: bool, payload: &str) {
        if let Some(client) = &self.client {
            let re = regex::Regex::new(r"^tln/([^/]+)/([^/]+)$").expect("Failed to regex");
            if let Some(captures) = re.captures(topic) {
                let name = &captures[1];
                let key = &captures[2];

                if let Err(e) = client
                    .publish(topic, QoS::AtLeastOnce, retain, payload)
                    .await
                {
                    self.log(MODULE, Warn, format!("[{MODULE}] Failed to publish topic (`{topic}`) payload (`{payload}`). Err: {e:?}")).await;
                } else {
                    utils::output_push(
                        MODULE,
                        &self.msg_tx,
                        &self.mode,
                        &self.gui_panel,
                        Info,
                        format!("[{MODULE}] -> pub::{key} {name} {payload}",),
                    )
                    .await;
                }
            }
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.log(MODULE, Info, format!("[{MODULE}] show")).await;
    }

    async fn handle_cmd_publish(&mut self, cmd_parts: &[String]) {
        if let (Some(retain), Some(key), Some(payload)) =
            (cmd_parts.get(3), cmd_parts.get(4), cmd_parts.get(5))
        {
            let retain = retain == "true";
            self.publish(&format!("tln/{}/{key}", cfg::name()), retain, payload)
                .await;
        }
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
                    ACTION_INIT => {
                        let shutdown_rx = self.shutdown_tx.subscribe();
                        self.handle_cmd_init(&cmd_parts, cmd, shutdown_rx).await;
                    }
                    "restart" => {
                        let shutdown_rx = self.shutdown_tx.subscribe();
                        self.start_mqtt(shutdown_rx).await;
                    }
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_PUBLISH => self.handle_cmd_publish(&cmd_parts).await,
                    ACTION_ARROW => (),
                    _ => {
                        self.log(
                            MODULE,
                            Warn,
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
                    Warn,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}

async fn process_event(
    msg_tx: &Sender<Msg>,
    mode: &Mode,
    gui_panel: &str,
    event: Result<Event, rumqttc::ConnectionError>,
) -> bool {
    match event {
        Ok(Event::Incoming(Incoming::Publish(publish))) => {
            process_event_publish(msg_tx, mode, gui_panel, &publish).await;
        }
        Ok(_) => { /* 其他事件略過 */ }
        Err(e) => {
            utils::output_push(
                MODULE,
                msg_tx,
                mode,
                gui_panel,
                Warn,
                format!("[{MODULE}] ❌ Event loop 錯誤: {e:?}"),
            )
            .await;
            return true;
        }
    }
    false
}

async fn process_event_publish(
    msg_tx: &Sender<Msg>,
    mode: &Mode,
    gui_panel: &str,
    publish: &Publish,
) {
    let topic = &publish.topic;
    let re = regex::Regex::new(r"^tln/([^/]+)/([^/]+)$").expect("Failed to regex");

    if let Some(captures) = re.captures(topic) {
        let name = &captures[1];
        let key = &captures[2];
        let payload = std::str::from_utf8(&publish.payload).expect("Failed to parse payload");

        match key {
            ACTION_ONBOARD | ACTION_VERSION | ACTION_TAILSCALE_IP | ACTION_TEMPERATURE
            | ACTION_APP_UPTIME => {
                utils::output_push(
                    MODULE,
                    msg_tx,
                    mode,
                    gui_panel,
                    Info,
                    format!("[{MODULE}] <- pub::{key} {name} {payload}"),
                )
                .await;

                let msg = Msg {
                    ts: utils::ts(),
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!("p devices {key} {name} {payload}"),
                    }),
                };
                let _ = msg_tx.send(msg).await;
            }
            _ => {
                utils::output_push(
                    MODULE,
                    msg_tx,
                    mode,
                    gui_panel,
                    Warn,
                    format!("[{MODULE}] <- pub::{key} {name} {payload}"),
                )
                .await;
            }
        }
    }
}
