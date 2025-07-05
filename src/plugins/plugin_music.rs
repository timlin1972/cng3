use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_INIT, ACTION_SHOW, Cmd, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, yt_dlp::YtDlp};

const MODULE: &str = "music";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    gui_panel: String,
    yt_dlp: YtDlp,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        utils::log::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            gui_panel: "infos".to_string(),
            yt_dlp: YtDlp::new().await,
        }
    }

    async fn handle_cmd_init(&mut self) {
        self.yt_dlp.init().await;
        self.info(MODULE, format!("[{MODULE}] init")).await;
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("[{MODULE}] show")).await;
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
                    ACTION_INIT => self.handle_cmd_init().await,
                    ACTION_SHOW => self.handle_cmd_show().await,
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
