use async_trait::async_trait;
use log::Level::{Info, Warn};
use tokio::sync::broadcast;

use crate::messages::{ACTION_SHOW, Cmd, Data, Log, Msg};
use crate::plugins::{
    plugin_cli, plugin_devices, plugin_infos, plugin_log, plugin_monitor, plugin_mqtt,
    plugin_music, plugin_nas, plugin_panels, plugin_scripts, plugin_system, plugin_weather,
};
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
            ts: utils::time::ts(),
            module: module.to_string(),
            data: Data::Log(Log { level, msg }),
        };
        let _ = self.send(msg).await;
    }

    async fn info(&self, module: &str, msg: String) {
        let _ = self.log(module, Info, msg).await;
    }

    async fn warn(&self, module: &str, msg: String) {
        let _ = self.log(module, Warn, msg).await;
    }

    async fn cmd(&self, module: &str, cmd: String) {
        let msg = Msg {
            ts: utils::time::ts(),
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
            Box::new(plugin_log::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_scripts::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_cli::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_system::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_panels::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_mqtt::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_devices::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_infos::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_nas::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_monitor::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_weather::PluginUnit::new(msg_tx.clone(), shutdown_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
            Box::new(plugin_music::PluginUnit::new(msg_tx.clone()).await)
                as Box<dyn Plugin + Send + Sync>,
        ];

        utils::msg::log_new(&msg_tx, MODULE).await;

        Self { msg_tx, plugins }
    }

    async fn log(&self, level: log::Level, msg: String) {
        let msg = Msg {
            ts: utils::time::ts(),
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

    async fn my_handle_cmd(&self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(2) {
            #[allow(clippy::single_match)]
            match action.as_str() {
                ACTION_SHOW => {
                    self.info(format!("{MODULE:<12}")).await;
                    for plugin in &self.plugins {
                        self.info(format!("{:<12}", plugin.name())).await;
                    }
                }
                _ => (),
            }
        }
    }

    pub async fn handle_cmd(&mut self, msg: &Msg) {
        if let Data::Cmd(cmd) = &msg.data {
            let cmd_parts = shell_words::split(&cmd.cmd).expect("Failed to parse cmd.");
            if let Some(plugin_name) = cmd_parts.get(1) {
                #[allow(clippy::collapsible_else_if)]
                if plugin_name == MODULE {
                    self.my_handle_cmd(&cmd_parts).await;
                } else {
                    if let Some(plugin) = self.get_plugin_mut(plugin_name) {
                        plugin.handle_cmd(msg).await;
                    } else {
                        self.warn(format!(
                            "[{MODULE}] Unknown plugin name (`{plugin_name}`) for cmd `{}`.",
                            cmd.cmd
                        ))
                        .await;
                    }
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
