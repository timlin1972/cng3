use async_trait::async_trait;
use log::Level::{Info, Warn};
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_ARROW, ACTION_LOG, Cmd, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils;

const MODULE: &str = "log";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    gui_panel: String,
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
                    ACTION_LOG => {
                        let ts = msg.ts;
                        if let (Some(level), Some(msg)) = (cmd_parts.get(3), cmd_parts.get(4)) {
                            if self.gui_panel.is_empty() {
                                println!("{} [{level}] {msg}", utils::ts_str(ts));
                            } else {
                                let msg = Msg {
                                    ts: utils::ts(),
                                    module: MODULE.to_string(),
                                    data: Data::Cmd(Cmd {
                                        cmd: format!(
                                            "p panels output_push {} '{} [{level}] {msg}'",
                                            self.gui_panel,
                                            utils::ts_str(ts)
                                        ),
                                    }),
                                };
                                let _ = self.msg_tx.send(msg).await;
                            }
                        } else {
                            self.log(
                                MODULE,
                                Warn,
                                format!("[{MODULE}] Missing level/msg for cmd `{}`.", cmd.cmd),
                            )
                            .await;
                        }
                    }
                    "gui" => {
                        if let Some(gui_panel) = cmd_parts.get(3) {
                            self.gui_panel = gui_panel.to_string();
                        }
                    }
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
