use std::fs::File;
use std::io::{self, BufRead};

use async_trait::async_trait;
use log::Level::{Info, Warn};
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_INIT, ACTION_SHOW, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils;

const MODULE: &str = "scripts";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    scripts_filename: Option<String>,
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
            scripts_filename: None,
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
                        if let Some(scripts_filename) = cmd_parts.get(3) {
                            if let Ok(file) = File::open(scripts_filename) {
                                let reader = io::BufReader::new(file);

                                for line in reader.lines().map_while(Result::ok) {
                                    self.cmd(MODULE, line).await;
                                }

                                self.log(
                                    MODULE,
                                    Info,
                                    format!("[{MODULE}] init script (`{scripts_filename}`)"),
                                )
                                .await;
                            } else {
                                self.log(
                                    MODULE,
                                    Warn,
                                    format!(
                                        "[{MODULE}] init script (`{scripts_filename}`) not found!"
                                    ),
                                )
                                .await;
                            }
                            self.scripts_filename = Some(scripts_filename.to_string());
                        }
                    }
                    ACTION_SHOW => {
                        if let Some(scripts_filename) = &self.scripts_filename {
                            if let Ok(file) = File::open(scripts_filename) {
                                let reader = io::BufReader::new(file);

                                for line in reader.lines().map_while(Result::ok) {
                                    self.log(MODULE, Info, format!("[{MODULE}] {line}")).await;
                                }
                            } else {
                                self.log(
                                    MODULE,
                                    Warn,
                                    format!(
                                        "[{MODULE}] init script (`{scripts_filename}`) not found!"
                                    ),
                                )
                                .await;
                            }
                        }
                    }
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
