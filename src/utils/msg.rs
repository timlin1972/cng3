use tokio::sync::mpsc::Sender;

use crate::messages::{Cmd, Data, Log, Msg};
use crate::utils;

pub async fn log_new(msg_tx: &Sender<Msg>, module: &str) {
    let msg = Msg {
        ts: utils::time::ts(),
        module: module.to_string(),
        data: Data::Log(Log {
            level: log::Level::Info,
            msg: format!("[{module}] new"),
        }),
    };
    let _ = msg_tx.send(msg).await;
}

pub async fn log_info(msg_tx: &Sender<Msg>, module: &str, msg: String) {
    let msg = Msg {
        ts: utils::time::ts(),
        module: module.to_string(),
        data: Data::Log(Log {
            level: log::Level::Info,
            msg,
        }),
    };
    let _ = msg_tx.send(msg).await;
}

pub async fn log_warn(msg_tx: &Sender<Msg>, module: &str, msg: String) {
    let msg = Msg {
        ts: utils::time::ts(),
        module: module.to_string(),
        data: Data::Log(Log {
            level: log::Level::Warn,
            msg,
        }),
    };
    let _ = msg_tx.send(msg).await;
}

pub async fn cmd(msg_tx: &Sender<Msg>, module: &str, cmd: String) {
    let msg = Msg {
        ts: utils::time::ts(),
        module: module.to_string(),
        data: Data::Cmd(Cmd { cmd }),
    };
    let _ = msg_tx.send(msg).await;
}
