use log::Level::{Info, Warn};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::plugins::plugins_main::Plugins;
use crate::utils;

const MODULE: &str = "messages";

pub const ACTION_APP_UPTIME: &str = "app_uptime";
pub const ACTION_ARROW: &str = "arrow";
pub const ACTION_CREATE: &str = "create";
pub const ACTION_DEVICES: &str = "devices";
pub const ACTION_FILE_MODIFY: &str = "file_modify";
pub const ACTION_FILE_REMOVE: &str = "file_remove";
pub const ACTION_INIT: &str = "init";
pub const ACTION_LOG: &str = "log";
pub const ACTION_NAS_STATE: &str = "nas_state";
pub const ACTION_ONBOARD: &str = "onboard";
pub const ACTION_PUBLISH: &str = "publish";
pub const ACTION_SHOW: &str = "show";
pub const ACTION_TAILSCALE_IP: &str = "tailscale_ip";
pub const ACTION_TEMPERATURE: &str = "temperature";
pub const ACTION_VERSION: &str = "version";

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
    msg_tx: Sender<Msg>,
}

impl Messages {
    pub async fn new(
        msg_tx: Sender<Msg>,
        mut msg_rx: Receiver<Msg>,
        shutdown_notify: broadcast::Sender<()>,
    ) -> Self {
        let mut plugins = Plugins::new(msg_tx.clone(), shutdown_notify.clone()).await;

        let msg_tx_clone = msg_tx.clone();

        tokio::spawn(async move {
            loop {
                let shutdown_notify_clone = shutdown_notify.clone();
                let mut shutdown_rx = shutdown_notify_clone.subscribe();

                tokio::select! {
                    maybe_msg = msg_rx.recv() => {
                        if let Some(msg) = maybe_msg {
                            match msg.data {
                                Data::Log(ref log) => parse_log(log, msg.ts, &msg.module, &msg_tx_clone).await,
                                Data::Cmd(ref _cmd) => parse_cmd(&msg, &msg_tx_clone, &mut plugins, shutdown_notify_clone).await,
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
            ts: utils::time::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        let _ = msg_tx.send(msg).await;

        Self { msg_tx }
    }

    pub async fn send(&self, msg: Msg) {
        let _ = self.msg_tx.send(msg).await;
    }
}

async fn parse_log(log: &Log, ts: u64, module: &str, msg_tx: &Sender<Msg>) {
    let msg = Msg {
        ts,
        module: module.to_string(),
        data: Data::Cmd(Cmd {
            cmd: format!("p log {ACTION_LOG} {} '{}'", log.level, log.msg),
        }),
    };
    let _ = msg_tx.send(msg).await;
}

async fn parse_cmd(
    msg: &Msg,
    msg_tx: &Sender<Msg>,
    plugins: &mut Plugins,
    shutdown_notify: broadcast::Sender<()>,
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
            "p" => plugins.handle_cmd(msg).await,
            "exit" | "q" | "quit" => {
                let _ = shutdown_notify.send(());
            }
            _ => {
                let msg = Msg {
                    ts: utils::time::ts(),
                    module: MODULE.to_string(),
                    data: Data::Log(Log {
                        level: Warn,
                        msg: format!("[{MODULE}] Unknown command: {command}"),
                    }),
                };
                let _ = msg_tx.send(msg).await;
            }
        }
    }
}
