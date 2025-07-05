use log::Level::Info;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{Receiver, Sender};

use crate::messages::{ACTION_INIT, Cmd, Data, Log, Messages, Msg};
use crate::utils;

const MODULE: &str = "app";

pub struct App {
    msgs: Messages,
    scripts_filename: String,
}

impl App {
    pub async fn new(
        msg_tx: Sender<Msg>,
        msg_rx: Receiver<Msg>,
        shutdown_notify: broadcast::Sender<()>,
        scripts_filename: String,
    ) -> Self {
        let app = Self {
            msgs: Messages::new(msg_tx, msg_rx, shutdown_notify).await,
            scripts_filename,
        };

        // log
        let msg = Msg {
            ts: utils::time::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        app.msgs.send(msg).await;

        app
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let msg = Msg {
            ts: utils::time::ts(),
            module: MODULE.to_string(),
            data: Data::Cmd(Cmd {
                cmd: format!("p scripts {ACTION_INIT} {}", self.scripts_filename),
            }),
        };
        self.msgs.send(msg).await;

        Ok(())
    }
}
