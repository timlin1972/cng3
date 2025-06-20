use async_trait::async_trait;
use tokio::sync::broadcast;

use crate::messages::{Cmd, Data, Log, Msg};
use crate::plugins::{plugin_cli, plugin_log, plugin_panels, plugin_scripts, plugin_system};
use crate::utils;

const MODULE: &str = "plugins";

#[async_trait]
pub trait Plugin {
    fn name(&self) -> &str;

    async fn handle_cmd(&mut self, msg: &Msg) {
        panic!("cmd: Unhandled msg ({msg:?})")
    }

    async fn send(&self, msg: Msg) {
        panic!("send: Unhandled msg ({msg:?})")
    }

    async fn log(&self, module: &str, level: log::Level, msg: String) {
        let msg = Msg {
            ts: utils::ts(),
            module: module.to_string(),
            data: Data::Log(Log { level, msg }),
        };
        let _ = self.send(msg).await;
    }

    async fn cmd(&self, module: &str, cmd: String) {
        let msg = Msg {
            ts: utils::ts(),
            module: module.to_string(),
            data: Data::Cmd(Cmd { cmd }),
        };
        let _ = self.send(msg).await;
    }
}

pub struct Plugins {
    msg_tx: tokio::sync::mpsc::Sender<Msg>,
    plugins: Vec<Box<dyn Plugin + Send + Sync>>,
}

impl Plugins {
    pub async fn new(
        msg_tx: tokio::sync::mpsc::Sender<Msg>,
        shutdown_tx: broadcast::Sender<()>,
    ) -> Self {
        let plugins = vec![
            Box::new(plugin_log::Plugin::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_scripts::Plugin::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_cli::Plugin::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_system::Plugin::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_panels::Plugin::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
        ];

        let myself = Self { msg_tx, plugins };

        myself.info(format!("[{MODULE}] new")).await;

        myself
    }

    async fn log(&self, level: log::Level, msg: String) {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log { level, msg }),
        };
        let _ = self.msg_tx.send(msg).await;
    }

    async fn info(&self, msg: String) {
        self.log(log::Level::Info, msg).await;
    }

    async fn warn(&self, msg: String) {
        self.log(log::Level::Warn, msg).await;
    }

    pub async fn handle_cmd(&mut self, msg: &Msg) {
        if let Data::Cmd(cmd) = &msg.data {
            let cmd_parts = shell_words::split(&cmd.cmd).expect("Failed to parse cmd.");
            if let Some(plugin_name) = cmd_parts.get(1) {
                if let Some(plugin) = self.get_plugin_mut(plugin_name) {
                    plugin.handle_cmd(msg).await;
                } else {
                    self.warn(format!(
                        "[{MODULE}] Unknown plugin name (`{plugin_name}`) for cmd `{}`.",
                        cmd.cmd
                    ))
                    .await;
                }
            } else {
                self.warn(format!(
                    "[{MODULE}] Missing plugin name for cmd `{}`.",
                    cmd.cmd
                ))
                .await;
            }
        }
    }

    fn get_plugin_mut(&mut self, name: &str) -> Option<&mut Box<dyn Plugin + Send + Sync>> {
        self.plugins.iter_mut().find(|p| p.name() == name)
    }
}
