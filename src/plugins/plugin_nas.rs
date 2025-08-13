use std::{
    fs,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose;
use chrono::{DateTime, Utc};
use serde_json::json;
use tokio::sync::mpsc::Sender; // trait for `.encode()`

use crate::cfg;
use crate::consts::{self, NAS_FOLDER, WEB_PORT};
use crate::messages::{
    ACTION_DEVICES, ACTION_FILE_MODIFY, ACTION_FILE_REMOVE, ACTION_INIT, ACTION_NAS_STATE,
    ACTION_ONBOARD, ACTION_SELF_NAS_STATE, ACTION_SHOW, ACTION_TAILSCALE_IP, Cmd, Data, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{
    self,
    nas_info::{self, FileList, NasEvent, NasInfo, NasState, SyncAction},
};

const MODULE: &str = "nas";
const WAITING_FOR_NAS_SERVER_IP_DELAY: u64 = 3;

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    inited: bool,
    gui_panel: String,
    nas_server: String,
    nas_state: NasState,     // For client
    nas_infos: Vec<NasInfo>, // For server
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            inited: false,
            gui_panel: "infos".to_string(),
            nas_server: String::new(),
            nas_state: NasState::Unsync,
            nas_infos: vec![],
        }
    }

    async fn update_infos_client_nas_state(&mut self) {
        // update infos
        self.cmd(
            MODULE,
            format!(
                "p {} nas {ACTION_NAS_STATE} {:?}",
                self.gui_panel, self.nas_state
            ),
        )
        .await;
    }

    async fn handle_nas_event_server(&mut self, name: &String, nas_event: &NasEvent) {
        // only care the nas_event from client
        #[allow(clippy::collapsible_if)]
        if *name != self.nas_server {
            if let Some(nas_info) = self
                .nas_infos
                .iter_mut()
                .find(|nas_info| nas_info.name == *name)
            {
                match nas_info.nas_state {
                    NasState::Unsync => {
                        match nas_event {
                            NasEvent::Onboard => (),  // keep unsync
                            NasEvent::Offboard => (), // keep unsync
                        }
                    }
                    NasState::Syncing => {
                        match nas_event {
                            NasEvent::Onboard => (), // keep syncing
                            NasEvent::Offboard => nas_info.nas_state = NasState::Unsync,
                        }
                    }
                    NasState::Synced => {
                        match nas_event {
                            NasEvent::Onboard => (), // keep syncing
                            NasEvent::Offboard => nas_info.nas_state = NasState::Unsync,
                        }
                    }
                    _ => todo!(),
                }

                // update infos
                let nas_state_clone = nas_info.nas_state.clone();
                self.cmd(
                    MODULE,
                    format!(
                        "p {} nas {ACTION_NAS_STATE} {name} {nas_state_clone:?}",
                        self.gui_panel
                    ),
                )
                .await;
            }
        }
    }

    async fn get_nas_server_ip(&self) -> Option<String> {
        if let Some(nas_info) = self
            .nas_infos
            .iter()
            .find(|nas_info| nas_info.name == *self.nas_server)
        {
            nas_info.tailscale_ip.clone()
        } else {
            None
        }
    }

    async fn handle_cmd_self_nas_state(&mut self, cmd_parts: &[String]) {
        if let Some(nas_state) = cmd_parts.get(3) {
            match nas_state.as_str() {
                "Synced" => {
                    self.nas_state = NasState::Synced;
                    self.update_infos_client_nas_state().await;
                }
                "Syncing" => {
                    self.nas_state = NasState::Syncing;
                    self.update_infos_client_nas_state().await;
                }
                _ => todo!(),
            }
        }
    }

    async fn handle_nas_event_client_in_state_unsync_onboard(&mut self) {
        // if nas_server_ip ready?
        let nas_server_ip = self.get_nas_server_ip().await;
        if nas_server_ip.is_none() {
            let msg_tx_clone = self.msg_tx.clone();
            let nas_server_clone = self.nas_server.clone();
            tokio::spawn(async move {
                utils::msg::log_info(&msg_tx_clone, MODULE, format!("[{MODULE}] {}: Unknown IP, re-onboard in {WAITING_FOR_NAS_SERVER_IP_DELAY} seconds.", &nas_server_clone)).await;
                tokio::time::sleep(tokio::time::Duration::from_secs(
                    WAITING_FOR_NAS_SERVER_IP_DELAY,
                ))
                .await;

                // re-onboard
                utils::msg::cmd(
                    &msg_tx_clone,
                    MODULE,
                    format!("p nas {ACTION_DEVICES} onboard {nas_server_clone} '1'"),
                )
                .await;
            });

            return;
        }

        let nas_server_ip = nas_server_ip.unwrap();
        let msg_tx_clone = self.msg_tx.clone();
        let nas_server_clone = self.nas_server.clone();
        tokio::spawn(async move {
            loop {
                // get file_list
                let file_list = FileList::new(consts::NAS_FOLDER).await;

                // send to server
                utils::msg::log_info(
                    &msg_tx_clone,
                    MODULE,
                    format!("[{MODULE}] {}: Check hash.", &nas_server_clone),
                )
                .await;

                let client = reqwest::Client::new();
                let json: serde_json::Value = client
                    .post(format!("http://{}:{WEB_PORT}/check_hash", &nas_server_ip))
                    .json(&json!({
                        "data": {
                            "name": cfg::name(),
                            "hash_str": file_list.hash_str,
                        }
                    }))
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap()
                    .parse()
                    .unwrap();

                let result = json["data"]["result"].as_u64().unwrap();

                if result == 0 {
                    utils::msg::log_info(
                        &msg_tx_clone,
                        MODULE,
                        format!("[{MODULE}] {}: Hash matched. Synced.", &nas_server_clone),
                    )
                    .await;
                    utils::msg::cmd(
                        &msg_tx_clone,
                        MODULE,
                        format!("p nas {ACTION_SELF_NAS_STATE} Synced"),
                    )
                    .await;
                    break;
                } else {
                    utils::msg::log_info(
                        &msg_tx_clone,
                        MODULE,
                        format!(
                            "[{MODULE}] {}: Hash mismatched. Start to sync.",
                            &nas_server_clone
                        ),
                    )
                    .await;
                    utils::msg::cmd(
                        &msg_tx_clone,
                        MODULE,
                        format!("p nas {ACTION_SELF_NAS_STATE} Syncing"),
                    )
                    .await;

                    let file_list_server = json["data"]["file_list"].clone();
                    let file_list_server: FileList =
                        serde_json::from_value(file_list_server).unwrap();

                    let actions =
                        nas_info::compare_and_generate_actions(&file_list_server, &file_list);
                    for action in &actions {
                        match action {
                            SyncAction::GetFile { filename, mtime: _ } => {
                                let client = reqwest::Client::new();
                                let resp: serde_json::Value = client
                                    .post(format!("http://{}:{WEB_PORT}/download", &nas_server_ip))
                                    .json(&json!({
                                        "data": {
                                            "filename": filename,
                                        }
                                    }))
                                    .send()
                                    .await
                                    .unwrap()
                                    .text()
                                    .await
                                    .unwrap()
                                    .parse()
                                    .unwrap();

                                let filename = resp["data"]["filename"].as_str().unwrap();
                                let content = resp["data"]["content"].as_str().unwrap();
                                let mtime = resp["data"]["mtime"].as_str().unwrap();

                                let _ = nas_info::write_file(filename, content, mtime).await;

                                utils::msg::log_info(
                                    &msg_tx_clone,
                                    MODULE,
                                    format!(
                                        "[{MODULE}] GET `{filename}` from {}",
                                        &nas_server_clone
                                    ),
                                )
                                .await;
                            }
                            SyncAction::PutFile { filename, mtime: _ } => {
                                put_file(
                                    &msg_tx_clone,
                                    &nas_server_ip,
                                    &nas_server_clone,
                                    filename,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        });
    }

    async fn handle_nas_event_client_in_state_unsync(&mut self, nas_event: &NasEvent) {
        match nas_event {
            NasEvent::Onboard => self.handle_nas_event_client_in_state_unsync_onboard().await,
            NasEvent::Offboard => (), // keep unsync
        }
    }

    async fn handle_nas_event_client_in_state_sync(&mut self, nas_event: &NasEvent) {
        match nas_event {
            NasEvent::Onboard => (), // keep sync
            NasEvent::Offboard => {
                self.nas_state = NasState::Unsync;
                self.update_infos_client_nas_state().await;
            }
        }
    }

    async fn handle_nas_event_client(&mut self, name: &String, nas_event: &NasEvent) {
        // only care the nas_event from server
        if *name == self.nas_server {
            match self.nas_state {
                NasState::Unsync => {
                    self.handle_nas_event_client_in_state_unsync(nas_event)
                        .await
                }
                NasState::Synced => self.handle_nas_event_client_in_state_sync(nas_event).await,
                _ => todo!(),
            }
        }
    }

    async fn handle_nas_event(&mut self, name: &String, nas_event: &NasEvent) {
        if self.nas_server == cfg::name() {
            self.handle_nas_event_server(name, nas_event).await;
        } else {
            self.handle_nas_event_client(name, nas_event).await;
        }
    }

    async fn handle_cmd_devices(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            let ts = utils::time::ts();
            match action.as_str() {
                ACTION_ONBOARD => {
                    if let (Some(name), Some(onboard)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let onboard_str = onboard.clone();
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

                        // update infos
                        self.cmd(
                            MODULE,
                            format!("p {} nas onboard {name} {onboard_str}", self.gui_panel,),
                        )
                        .await;
                        self.update_infos_client_nas_state().await;

                        // handle_nas_event
                        self.handle_nas_event(
                            name,
                            if onboard {
                                &NasEvent::Onboard
                            } else {
                                &NasEvent::Offboard
                            },
                        )
                        .await;
                    }
                }
                ACTION_TAILSCALE_IP => {
                    #[allow(clippy::collapsible_if)]
                    if let (Some(name), Some(tailscale_ip)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(nas_info) = self
                            .nas_infos
                            .iter_mut()
                            .find(|nas_info| nas_info.name == *name)
                        {
                            nas_info.ts = ts;
                            nas_info.tailscale_ip = Some(tailscale_ip.to_string());
                        }
                    }
                }
                _ => (),
            }
        }
    }

    async fn handle_cmd_init(&mut self, cmd_parts: &[String]) {
        if self.inited {
            return;
        }
        self.inited = true;

        let _ = fs::create_dir_all(NAS_FOLDER);

        if let Some(nas_server) = cmd_parts.get(3) {
            self.nas_server = nas_server.to_string();

            // update infos
            let msg = Msg {
                ts: utils::time::ts(),
                module: MODULE.to_string(),
                data: Data::Cmd(Cmd {
                    cmd: format!("p {} nas nas_server {nas_server}", self.gui_panel,),
                }),
            };
            let _ = self.msg_tx.send(msg).await;
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("Nas Server: {}", self.nas_server))
            .await;
        self.info(MODULE, format!("Nas State: {:?}", self.nas_state))
            .await;
        self.info(
            MODULE,
            format!("{:<12} {:<7} {:<16}", "Name", "Onboard", "Tailscale IP"),
        )
        .await;
        for nas_info in &self.nas_infos {
            self.info(
                MODULE,
                format!(
                    "{:<12} {:<7} {:<16}",
                    nas_info.name,
                    if nas_info.onboard { "on " } else { "off" },
                    nas_info.tailscale_ip.clone().unwrap_or("n/a".to_string())
                ),
            )
            .await;
        }
    }

    async fn handle_cmd_nas_state(&mut self, cmd_parts: &[String]) {
        #[allow(clippy::collapsible_if)]
        if let (Some(name), Some(nas_state)) = (cmd_parts.get(3), cmd_parts.get(4)) {
            if let Some(nas_info) = self
                .nas_infos
                .iter_mut()
                .find(|nas_info| nas_info.name == *name)
            {
                match nas_state.as_str() {
                    "Synced" => nas_info.nas_state = NasState::Synced,
                    "Syncing" => nas_info.nas_state = NasState::Syncing,
                    _ => todo!(),
                }

                // update infos
                let nas_info_nas_state = nas_info.nas_state.clone();
                self.cmd(
                    MODULE,
                    format!(
                        "p {} nas {ACTION_NAS_STATE} {name} {nas_info_nas_state:?}",
                        self.gui_panel
                    ),
                )
                .await;
            }
        }
    }

    async fn remove_file(&self, remote_ip: &str, remote_name: &str, filename: &str) {
        let client = reqwest::Client::new();
        let _ = client
            .post(format!("http://{remote_ip}:{WEB_PORT}/remove"))
            .json(&json!({
                "data": {
                    "filename": filename,
                }
            }))
            .send()
            .await;

        self.info(
            MODULE,
            format!("[{MODULE}] REMOVE `{filename}` to {remote_name}"),
        )
        .await;
    }

    async fn handle_cmd_file_modify(&mut self, cmd_parts: &[String]) {
        if let Some(filename) = cmd_parts.get(3) {
            let filename_bytes = general_purpose::STANDARD
                .decode(filename)
                .expect("Failed to decode");
            let filename = String::from_utf8(filename_bytes).expect("Invalid UTF-8");

            // server
            #[allow(clippy::collapsible_else_if)]
            if self.nas_server == cfg::name() {
                // send to all clients except me
                for nas_info in &self.nas_infos {
                    if nas_info.name != self.nas_server && nas_info.tailscale_ip.is_some() {
                        put_file(
                            &self.msg_tx,
                            &nas_info.tailscale_ip.clone().unwrap(),
                            &nas_info.name,
                            &filename,
                        )
                        .await;
                    }
                }
            }
            // client
            else {
                if self.nas_state == NasState::Synced {
                    let nas_server_ip = self.get_nas_server_ip().await.unwrap(); // must NOT be None
                    put_file(&self.msg_tx, &nas_server_ip, &self.nas_server, &filename).await;
                }
            }
        }
    }

    async fn handle_cmd_file_remove(&mut self, cmd_parts: &[String]) {
        if let Some(filename) = cmd_parts.get(3) {
            let filename_bytes = general_purpose::STANDARD
                .decode(filename)
                .expect("Failed to decode");
            let filename = String::from_utf8(filename_bytes).expect("Invalid UTF-8");

            // server
            #[allow(clippy::collapsible_else_if)]
            if self.nas_server == cfg::name() {
                // send to all clients except me
                for nas_info in &self.nas_infos {
                    if nas_info.name != self.nas_server && nas_info.tailscale_ip.is_some() {
                        self.remove_file(
                            &nas_info.tailscale_ip.clone().unwrap(),
                            &nas_info.name,
                            &filename,
                        )
                        .await;
                    }
                }
            }
            // client
            else {
                if self.nas_state == NasState::Synced {
                    let nas_server_ip = self.get_nas_server_ip().await.unwrap(); // must NOT be None
                    self.remove_file(&nas_server_ip, &self.nas_server, &filename)
                        .await;
                }
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
                    ACTION_INIT => self.handle_cmd_init(&cmd_parts).await,
                    ACTION_DEVICES => self.handle_cmd_devices(&cmd_parts).await,
                    ACTION_NAS_STATE => self.handle_cmd_nas_state(&cmd_parts).await,
                    ACTION_FILE_MODIFY => self.handle_cmd_file_modify(&cmd_parts).await,
                    ACTION_FILE_REMOVE => self.handle_cmd_file_remove(&cmd_parts).await,
                    ACTION_SELF_NAS_STATE => self.handle_cmd_self_nas_state(&cmd_parts).await,
                    _ => {
                        self.info(
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
                self.info(
                    MODULE,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}

async fn put_file(msg_tx: &Sender<Msg>, remote_ip: &str, remote_name: &str, filename: &str) {
    let path = Path::new(filename);
    if !path.exists() {
        utils::msg::log_warn(
            msg_tx,
            MODULE,
            format!("[{MODULE}] PUT `{filename}` failed. Fild not found."),
        )
        .await;
    } else {
        let file_path = PathBuf::from(filename);

        let bytes = fs::read(&file_path).unwrap();
        let hash_str = nas_info::hash_str(&String::from_utf8_lossy(&bytes));

        let client = reqwest::Client::new();
        let json: serde_json::Value = client
            // let json = client
            .post(format!("http://{remote_ip}:{WEB_PORT}/verify_hash"))
            .json(&json!({
                "data": {
                    "filename": filename,
                    "hash_str": hash_str,
                }
            }))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .parse()
            .unwrap();

        let result = json["data"]["result"].as_u64().unwrap();
        if result == 0 {
            utils::msg::log_info(
                msg_tx,
                MODULE,
                format!("[{MODULE}] PUT `{filename}` to {remote_name} ignored. Same."),
            )
            .await;
        } else {
            let mtime = fs::metadata(&file_path)
                .and_then(|meta| meta.modified())
                .map(|time| DateTime::<Utc>::from(time).to_rfc3339())
                .unwrap_or_else(|_| Utc::now().to_rfc3339());
            let encoded = general_purpose::STANDARD.encode(&bytes);

            let client = reqwest::Client::new();
            let _ = client
                .post(format!("http://{remote_ip}:{WEB_PORT}/upload"))
                .json(&json!({
                    "data": {
                        "filename": filename,
                        "content": encoded,
                        "mtime": mtime,
                    }
                }))
                .send()
                .await;

            utils::msg::log_info(
                msg_tx,
                MODULE,
                format!("[{MODULE}] PUT `{filename}` to {remote_name}"),
            )
            .await;
        }
    }
}
