use async_trait::async_trait;
use log::Level::Info;
use tokio::sync::mpsc::Sender;

use crate::messages::{
    ACTION_DEVICES, ACTION_ONBOARD, ACTION_PUBLISH, ACTION_SHOW, ACTION_TAILSCALE_IP,
    ACTION_TEMPERATURE, ACTION_VERSION, Cmd, Data, Log, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, DevInfo};

const MODULE: &str = "devices";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    devices: Vec<DevInfo>,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
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
            devices: vec![],
        }
    }

    async fn handle_cmd_show(&mut self) {
        for device in &self.devices {
            self.info(MODULE, format!("[{MODULE}] {}", device.name))
                .await;

            // Last update
            self.info(
                MODULE,
                format!(
                    "[{MODULE}]     Last update: {}",
                    utils::ts_str_full(device.ts)
                ),
            )
            .await;

            // onboard
            self.info(
                MODULE,
                format!(
                    "[{MODULE}]     Onboard: {}",
                    utils::onboard_str(device.onboard)
                ),
            )
            .await;

            // version
            self.info(
                MODULE,
                format!(
                    "[{MODULE}]     Version: {}",
                    device.version.clone().unwrap_or("n/a".to_string())
                ),
            )
            .await;

            // tailscale_ip
            self.info(
                MODULE,
                format!(
                    "[{MODULE}]     IP: {}",
                    device.tailscale_ip.clone().unwrap_or("n/a".to_string())
                ),
            )
            .await;

            // temperature
            let temperature = if let Some(t) = device.temperature {
                format!("{:.1}°C", t)
            } else {
                "n/a".to_owned()
            };

            self.info(
                MODULE,
                format!("[{MODULE}]     Temperature: {temperature}",),
            )
            .await;
        }
    }

    async fn handle_cmd_onboard(&mut self, cmd_parts: &[String]) {
        if let (Some(name), Some(onboard)) = (cmd_parts.get(3), cmd_parts.get(4)) {
            let ts = utils::ts();
            let onbard_str = onboard.clone();
            let onboard = onboard == "1";

            let changed =
                if let Some(device) = self.devices.iter_mut().find(|device| device.name == *name) {
                    let changed = onboard != device.onboard;

                    device.ts = ts;
                    device.onboard = onboard;
                    changed
                } else {
                    let device_add = DevInfo {
                        ts,
                        name: name.to_string(),
                        onboard,
                        version: None,
                        tailscale_ip: None,
                        temperature: None,
                    };
                    self.devices.push(device_add.clone());
                    true
                };

            if changed {
                self.info(
                    MODULE,
                    format!(
                        "[{MODULE}] {name} {} at {}",
                        utils::onboard_str(onboard),
                        utils::ts_str_full(ts),
                    ),
                )
                .await;

                // someone onboard, publish immediately
                if onboard {
                    self.send(Msg {
                        ts,
                        module: MODULE.to_string(),
                        data: Data::Cmd(Cmd {
                            cmd: format!("p system {ACTION_PUBLISH}"),
                        }),
                    })
                    .await;
                }
            }

            // update infos
            let msg = Msg {
                ts,
                module: MODULE.to_string(),
                data: Data::Cmd(Cmd {
                    cmd: format!("p infos {ACTION_DEVICES} onboard {name} {onbard_str}"),
                }),
            };
            let _ = self.msg_tx.send(msg).await;

            // update nas
            let msg = Msg {
                ts,
                module: MODULE.to_string(),
                data: Data::Cmd(Cmd {
                    cmd: format!("p nas {ACTION_DEVICES} onboard {name} {onbard_str}"),
                }),
            };
            let _ = self.msg_tx.send(msg).await;
        }
    }

    async fn handle_cmd_version(&mut self, cmd_parts: &[String]) {
        if let (Some(name), Some(version)) = (cmd_parts.get(3), cmd_parts.get(4)) {
            let ts = utils::ts();

            if let Some(device) = self.devices.iter_mut().find(|device| device.name == *name) {
                device.ts = ts;
                device.version = Some(version.to_string());

                // update infos
                let msg = Msg {
                    ts,
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!("p infos {ACTION_DEVICES} version {name} {version}"),
                    }),
                };
                let _ = self.msg_tx.send(msg).await;
            }
        }
    }

    async fn handle_cmd_tailscale_ip(&mut self, cmd_parts: &[String]) {
        if let (Some(name), Some(tailscale_ip)) = (cmd_parts.get(3), cmd_parts.get(4)) {
            let ts = utils::ts();

            if let Some(device) = self.devices.iter_mut().find(|device| device.name == *name) {
                device.ts = ts;
                device.tailscale_ip = Some(tailscale_ip.to_string());

                // update infos
                let msg = Msg {
                    ts,
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!(
                            "p infos {ACTION_DEVICES} {ACTION_TAILSCALE_IP} {name} {tailscale_ip}"
                        ),
                    }),
                };
                let _ = self.msg_tx.send(msg).await;

                // update nas
                let msg = Msg {
                    ts,
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!(
                            "p nas {ACTION_DEVICES} {ACTION_TAILSCALE_IP} {name} {tailscale_ip}"
                        ),
                    }),
                };
                let _ = self.msg_tx.send(msg).await;
            }
        }
    }

    async fn handle_cmd_temperature(&mut self, cmd_parts: &[String]) {
        if let (Some(name), Some(temperature)) = (cmd_parts.get(3), cmd_parts.get(4)) {
            let ts = utils::ts();

            if let Some(device) = self.devices.iter_mut().find(|device| device.name == *name) {
                device.ts = ts;
                device.temperature = Some(temperature.parse::<f32>().unwrap());

                // update infos
                let msg = Msg {
                    ts,
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!(
                            "p infos {ACTION_DEVICES} {ACTION_TEMPERATURE} {name} {temperature}"
                        ),
                    }),
                };
                let _ = self.msg_tx.send(msg).await;
            }
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
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_ONBOARD => self.handle_cmd_onboard(&cmd_parts).await,
                    ACTION_VERSION => self.handle_cmd_version(&cmd_parts).await,
                    ACTION_TAILSCALE_IP => self.handle_cmd_tailscale_ip(&cmd_parts).await,
                    ACTION_TEMPERATURE => self.handle_cmd_temperature(&cmd_parts).await,
                    _ => {
                        self.warn(
                            MODULE,
                            format!(
                                "[{MODULE}] Unknown action ({action}) for cmd `{}`.",
                                cmd.cmd
                            ),
                        )
                        .await;
                    }
                }
            } else {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}
