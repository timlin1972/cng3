use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;

use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_INIT, ACTION_SHOW, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils;

const MODULE: &str = "scripts";
const SCRIPTS_FILENAME: &str = "./init.scripts";

#[derive(Debug)]
pub struct Plugin {
    name: String,
    msg_tx: Sender<Msg>,
}

impl Plugin {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: log::Level::Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");

        Self {
            name: MODULE.to_owned(),
            msg_tx,
        }
    }
}

#[async_trait]
impl plugins_main::Plugin for Plugin {
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
                        let path = Path::new(SCRIPTS_FILENAME);
                        if let Ok(file) = File::open(path) {
                            let reader = io::BufReader::new(file);

                            for line in reader.lines().map_while(Result::ok) {
                                self.cmd(MODULE, line).await;
                            }
                        } else {
                            self.log(
                                MODULE,
                                log::Level::Warn,
                                format!("[{MODULE}] init script (`{SCRIPTS_FILENAME}`) not found!"),
                            )
                            .await;
                        }
                    }
                    ACTION_SHOW => {
                        let path = Path::new(SCRIPTS_FILENAME);
                        if let Ok(file) = File::open(path) {
                            let reader = io::BufReader::new(file);

                            for line in reader.lines().map_while(Result::ok) {
                                self.log(MODULE, log::Level::Info, format!("[{MODULE}] {line}"))
                                    .await;
                            }
                        } else {
                            self.log(
                                MODULE,
                                log::Level::Warn,
                                format!("[{MODULE}] init script (`{SCRIPTS_FILENAME}`) not found!"),
                            )
                            .await;
                        }
                    }
                    _ => {
                        self.log(
                            MODULE,
                            log::Level::Warn,
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
                    log::Level::Warn,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}
