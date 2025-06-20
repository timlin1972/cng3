use tokio::sync::broadcast;

use crate::messages::{Cmd, Data, Log, Messages, Msg};
use crate::utils;

const MODULE: &str = "app";

pub struct App {
    msgs: Messages,
}

impl App {
    pub async fn new(shutdown_tx: broadcast::Sender<()>) -> Self {
        let myself = Self {
            msgs: Messages::new(shutdown_tx).await,
        };

        myself.info(format!("[{MODULE}] new")).await;

        myself
    }

    async fn log(&self, level: log::Level, msg: String) {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log { level, msg }),
        };
        self.msgs.send(msg).await;
    }

    async fn info(&self, msg: String) {
        self.log(log::Level::Info, msg).await;
    }

    async fn cmd(&self, cmd: String) {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Cmd(Cmd { cmd }),
        };
        self.msgs.send(msg).await;
    }

    pub async fn run(&mut self) -> std::io::Result<()> {
        self.cmd("p scripts init".to_string()).await;

        Ok(())
    }
}
