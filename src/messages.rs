use std::sync::Arc;

use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, Sender};

use crate::plugins::plugins_main::Plugins;
use crate::utils;

const MODULE: &str = "messages";
const MSG_SIZE: usize = 4096;

pub const ACTION_CREATE: &str = "create";
pub const ACTION_INIT: &str = "init";
pub const ACTION_LOG: &str = "log";
pub const ACTION_SHOW: &str = "show";

#[derive(Debug)]
pub enum Data {
    Log(Log),
    Cmd(Cmd),
}

#[derive(Debug)]
pub struct Msg {
    pub ts: u64,
    pub module: String,
    pub data: Data,
}

#[derive(Debug, Clone)]
pub struct Log {
    pub level: log::Level,
    pub msg: String,
}

#[derive(Debug, Clone)]
pub struct Cmd {
    pub cmd: String,
}

pub struct Messages {
    pub msg_tx: Arc<mpsc::Sender<Msg>>,
}

impl Messages {
    pub async fn new(shutdown_tx: broadcast::Sender<()>) -> Self {
        let (msg_tx, mut msg_rx) = mpsc::channel::<Msg>(MSG_SIZE);

        let mut plugins = Plugins::new(msg_tx.clone(), shutdown_tx.clone()).await;

        let msg_tx = Arc::new(msg_tx.clone());
        let msg_tx_clone = Arc::clone(&msg_tx);

        tokio::spawn(async move {
            loop {
                let msg_tx_clone_clone = msg_tx_clone.clone();
                let shutdown_tx_clone = shutdown_tx.clone();
                let mut shutdown_rx = shutdown_tx_clone.subscribe();

                tokio::select! {
                    maybe_msg = msg_rx.recv() => {
                        if let Some(msg) = maybe_msg {
                            match msg.data {
                                Data::Log(ref _log) => {
                                    parse_log(&msg, msg_tx_clone_clone).await;
                                }
                                Data::Cmd(ref _cmd) => {
                                    parse_cmd(&msg, msg_tx_clone_clone, &mut plugins, shutdown_tx_clone).await;
                                }
                            }
                        } else {
                            break; // msg_rx channel closed
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        break;
                    }
                }
            }
        });

        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: log::Level::Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");

        Self { msg_tx }
    }

    pub async fn send(&self, msg: Msg) {
        self.msg_tx.send(msg).await.expect("Failed to send message");
    }
}

async fn parse_log(msg: &Msg, msg_tx: Arc<Sender<Msg>>) {
    if let Data::Log(log) = &msg.data {
        let msg = Msg {
            ts: msg.ts,
            module: msg.module.clone(),
            data: Data::Cmd(Cmd {
                cmd: format!("p log log {} '{}'", log.level, log.msg),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");
    }
}

async fn parse_cmd(
    msg: &Msg,
    msg_tx: Arc<Sender<Msg>>,
    plugins: &mut Plugins,
    shutdown_tx: broadcast::Sender<()>,
) {
    if let Data::Cmd(cmd) = &msg.data {
        let cmd = &cmd.cmd;
        let cmd = cmd
            .split_once('#')
            .map(|(before, _)| before.trim_end())
            .unwrap_or(cmd);
        let cmd_parts: Vec<&str> = cmd.split_whitespace().collect();
        if cmd_parts.is_empty() {
            return;
        }
        let command = cmd_parts[0];
        match command {
            "p" => {
                plugins.handle_cmd(msg).await;
            }
            "exit" | "q" => {
                let _ = shutdown_tx.send(());
            }
            _ => {
                let msg = Msg {
                    ts: utils::ts(),
                    module: MODULE.to_string(),
                    data: Data::Log(Log {
                        level: log::Level::Warn,
                        msg: format!("[{MODULE}] Unknown command: {command}"),
                    }),
                };
                msg_tx.send(msg).await.expect("Failed to send message");
            }
        }
    }
}
