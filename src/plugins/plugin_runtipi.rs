use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose;
use tokio::sync::mpsc::Sender;

use crate::cfg;
use crate::messages::{ACTION_ARROW, ACTION_FILE_MODIFY, ACTION_INIT, ACTION_SHOW, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils;

const MODULE: &str = "runtipi";
const RUNTIPI_MUSIC_FOLDER: &str = "~/runtipi/media/data/music/";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    runtipi_server: String,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            runtipi_server: String::new(),
        }
    }

    async fn handle_cmd_init(&mut self, cmd_parts: &[String]) {
        if let Some(runtipi_server) = cmd_parts.get(3) {
            self.runtipi_server = runtipi_server.to_string();
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("Runtipi Server: {}", self.runtipi_server))
            .await;
    }

    async fn handle_cmd_file_modify(&mut self, cmd_parts: &[String]) {
        // only for runtipi server
        if self.runtipi_server != cfg::name() {
            self.warn(
                MODULE,
                format!("[{MODULE}] Runtipi server is not me, cannot handle file modify action."),
            )
            .await;
            return;
        }

        if let Some(filename) = cmd_parts.get(3) {
            let filename_bytes = general_purpose::STANDARD
                .decode(filename)
                .expect("Failed to decode");
            let filename = String::from_utf8(filename_bytes).expect("Invalid UTF-8");

            // if filename starts with "./nas/music/"
            if filename.starts_with("./nas/music/") {
                // cp file to RUNTIPI_MUSIC_FOLDER use system command
                let cmd = format!("cp -f {} {}", filename, RUNTIPI_MUSIC_FOLDER);
                self.info(MODULE, format!("[{MODULE}] Running command: {cmd}"))
                    .await;
                if let Err(e) = std::process::Command::new("sh").arg("-c").arg(cmd).output() {
                    self.warn(MODULE, format!("[{MODULE}] Failed to copy file: {e}"))
                        .await;
                } else {
                    self.info(MODULE, format!("[{MODULE}] File copied successfully."))
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
                    ACTION_INIT => self.handle_cmd_init(&cmd_parts).await,
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_FILE_MODIFY => self.handle_cmd_file_modify(&cmd_parts).await,
                    ACTION_ARROW => (),
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
