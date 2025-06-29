use async_trait::async_trait;
use log::Level::{Info, Warn};
use tokio::sync::mpsc::Sender;

use crate::cfg;
use crate::messages::{
    ACTION_ARROW, ACTION_DEVICES, ACTION_NAS_STATE, ACTION_ONBOARD, ACTION_SHOW,
    ACTION_TAILSCALE_IP, ACTION_TEMPERATURE, ACTION_VERSION, Cmd, Data, Log, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, DevInfo, NasInfo, NasState};

const MODULE: &str = "infos";
const PAGES: u16 = 2;

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    gui_panel: String,
    devices: Vec<DevInfo>,
    nas_server: String,
    nas_state: NasState,     // For client
    nas_infos: Vec<NasInfo>, // For server
    page_idx: u16,
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
            gui_panel: String::new(),
            devices: vec![],
            nas_server: String::new(),
            nas_state: NasState::Unsync,
            nas_infos: vec![],
            page_idx: 0,
        }
    }

    async fn panel_output_update(&mut self) {
        // update sub_title
        let sub_title = format!(" - {}/{PAGES}", self.page_idx + 1);
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Cmd(Cmd {
                cmd: format!("p panels sub_title {} '{sub_title}'", self.gui_panel),
            }),
        };
        let _ = self.msg_tx.send(msg).await;

        let mut output = String::new();
        match self.page_idx {
            0 => {
                output = format!(
                    "{:<12} {:<7} {:<10} {:16} {:<7}",
                    "Name", "Onboard", "Version", "Tailscale IP", "Temper",
                );
                for device in &self.devices {
                    output += &format!(
                        "\n{:<12} {:<7} {:<10} {:16} {:<7}",
                        device.name,
                        utils::onboard_str(device.onboard),
                        device.version.clone().unwrap_or("n/a".to_string()),
                        device.tailscale_ip.clone().unwrap_or("n/a".to_string()),
                        utils::temperature_str(device.temperature)
                    );
                }
            }
            1 => match self.nas_server == cfg::name() {
                true => {
                    output = format!("{:<12} {:<7} {:10}", "Name", "Onboard", "NAS State");
                    for nas_info in &self.nas_infos {
                        output += &format!(
                            "\n{:<12} {:<7} {:10?}",
                            nas_info.name,
                            utils::onboard_str(nas_info.onboard),
                            nas_info.nas_state
                        );
                    }
                }
                false => {
                    output = format!("Nas State: {:?}", self.nas_state);
                }
            },
            _ => (),
        }

        utils::output_update_gui_simple(MODULE, &self.msg_tx, &self.gui_panel, output).await;
    }

    async fn handle_cmd_devices(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            let ts = utils::ts();
            match action.as_str() {
                ACTION_ONBOARD => {
                    if let (Some(name), Some(onboard)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let onboard = onboard == "1";
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.onboard = onboard;
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
                        }
                    }
                }
                ACTION_VERSION => {
                    if let (Some(name), Some(version)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.version = Some(version.to_string());
                        }
                    }
                }
                ACTION_TAILSCALE_IP => {
                    if let (Some(name), Some(tailscale_ip)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.tailscale_ip = Some(tailscale_ip.to_string());
                        }
                    }
                }
                ACTION_TEMPERATURE => {
                    if let (Some(name), Some(temperature)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.temperature = Some(temperature.parse::<f32>().unwrap());
                        }
                    }
                }
                _ => (),
            }
            self.panel_output_update().await;
        }
    }

    async fn handle_cmd_nas(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            let ts = utils::ts();
            match action.as_str() {
                ACTION_ONBOARD => {
                    if let (Some(name), Some(onboard)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let onboard = onboard == "1";
                        if let Some(nas_info) = self
                            .nas_infos
                            .iter_mut()
                            .find(|nas_info| nas_info.name == *name)
                        {
                            nas_info.ts = ts;
                            nas_info.onboard = onboard;
                        } else {
                            let nas_info_add = NasInfo {
                                ts,
                                name: name.to_string(),
                                onboard,
                                nas_state: NasState::Unsync,
                                tailscale_ip: None,
                            };
                            self.nas_infos.push(nas_info_add.clone());
                        }
                    }
                }
                "nas_server" => {
                    if let Some(nas_server) = cmd_parts.get(4) {
                        self.nas_server = nas_server.clone();
                    }
                }
                ACTION_NAS_STATE => {
                    // server
                    if let (Some(name), Some(nas_state)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let nas_state = match nas_state.as_str() {
                            "Unsync" => NasState::Unsync,
                            "Syncing" => NasState::Syncing,
                            "Synced" => NasState::Synced,
                            "Err" => NasState::Err,
                            _ => todo!(),
                        };

                        if let Some(nas_info) = self
                            .nas_infos
                            .iter_mut()
                            .find(|nas_info| nas_info.name == *name)
                        {
                            nas_info.ts = ts;
                            nas_info.nas_state = nas_state;
                        }
                    }
                    // client
                    else if let Some(nas_state) = cmd_parts.get(4) {
                        match nas_state.as_str() {
                            "Unsync" => self.nas_state = NasState::Unsync,
                            "Syncing" => self.nas_state = NasState::Syncing,
                            "Synced" => self.nas_state = NasState::Synced,
                            "Err" => self.nas_state = NasState::Err,
                            _ => todo!(),
                        }
                    } else {
                        self.log(
                            MODULE,
                            Warn,
                            format!("[{MODULE}] Missing {ACTION_NAS_STATE} or name/{ACTION_NAS_STATE} for cmd `{cmd_parts:?}`."),
                        )
                        .await;
                    }
                }
                _ => {
                    self.log(
                        MODULE,
                        Warn,
                        format!("[{MODULE}] Unknown action ({action}) for cmd `{cmd_parts:?}`."),
                    )
                    .await
                }
            }
            self.panel_output_update().await;
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.log(
            MODULE,
            Info,
            format!("{:<12} {:<7} {:16}", "Name", "Onboard", "Tailscale IP"),
        )
        .await;
        for device in &self.devices {
            self.log(
                MODULE,
                Info,
                format!(
                    "{:<12} {:<7} {:16}",
                    device.name,
                    utils::onboard_str(device.onboard),
                    device.tailscale_ip.clone().unwrap_or("n/a".to_string())
                ),
            )
            .await;
        }

        self.log(
            MODULE,
            Info,
            format!("{:<12} {:<7} {:10}", "Name", "Onboard", "NAS State"),
        )
        .await;
        for nas_info in &self.nas_infos {
            self.log(
                MODULE,
                Info,
                format!(
                    "{:<12} {:<7} {:10?}",
                    nas_info.name,
                    utils::onboard_str(nas_info.onboard),
                    nas_info.nas_state
                ),
            )
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
                    "gui" => {
                        if let Some(gui_panel) = cmd_parts.get(3) {
                            self.gui_panel = gui_panel.to_string();
                        }
                    }
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_DEVICES => self.handle_cmd_devices(&cmd_parts).await,
                    "nas" => self.handle_cmd_nas(&cmd_parts).await,
                    ACTION_ARROW => {
                        if let Some(left_right) = cmd_parts.get(3) {
                            match left_right.as_str() {
                                "right" => self.page_idx = (self.page_idx + 1) % PAGES,
                                "left" => self.page_idx = (self.page_idx + PAGES - 1) % PAGES,
                                _ => (),
                            }
                        }

                        self.panel_output_update().await;
                    }
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
