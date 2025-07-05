use tokio::sync::mpsc::Sender;

use crate::messages::{Data, Log, Msg};
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
