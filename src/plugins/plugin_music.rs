use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

use crate::consts::MUSIC_FOLDER;
use crate::messages::{ACTION_INIT, ACTION_SHOW, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, ffmpeg::Ffmpeg, yt_dlp::YtDlp};

const MODULE: &str = "music";

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    inited: bool,
    yt_dlp: YtDlp,
    ffmpeg: Ffmpeg,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        let _ = std::fs::create_dir_all(MUSIC_FOLDER);

        utils::log::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            inited: false,
            yt_dlp: YtDlp::new(MUSIC_FOLDER.to_string()).await,
            ffmpeg: Ffmpeg::new().await,
        }
    }

    async fn handle_cmd_init(&mut self) {
        if self.inited {
            return;
        }

        let yt_dlp_installed = match self.yt_dlp.init().await {
            Ok(version) => {
                self.info(
                    MODULE,
                    format!("[{MODULE}] init with yt_dlp version {version}"),
                )
                .await;
                true
            }
            Err(e) => {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] Failed to init yt_dlp. Err: {e}"),
                )
                .await;
                false
            }
        };

        let ffmpeg_installed = match self.ffmpeg.init().await {
            Ok(version) => {
                self.info(
                    MODULE,
                    format!("[{MODULE}] init with ffmpeg version {version}"),
                )
                .await;
                true
            }
            Err(e) => {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] Failed to init ffmpeg. Err: {e}"),
                )
                .await;
                false
            }
        };

        self.inited = yt_dlp_installed && ffmpeg_installed;
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("[{MODULE}] inited: {:?}", self.inited))
            .await;
        self.info(
            MODULE,
            format!("[{MODULE}] yt_dlp version: {}", self.yt_dlp.version()),
        )
        .await;
        self.info(
            MODULE,
            format!("[{MODULE}] ffmpeg version: {}", self.ffmpeg.version()),
        )
        .await;
    }

    async fn my_handle_cmd_downlad(&mut self, cmd_parts: &[String]) {
        if !self.inited {
            self.warn(MODULE, format!("[{MODULE}] Not inited")).await;
            return;
        }

        if let Some(url) = cmd_parts.get(3) {
            self.info(MODULE, format!("[{MODULE}] download: {url}"))
                .await;

            match self.yt_dlp.download(url).await {
                Ok(_) => {
                    self.info(MODULE, format!("[{MODULE}] download: {url} ok."))
                        .await
                }
                Err(e) => {
                    self.info(MODULE, format!("[{MODULE}] download: {url} failed. {e}"))
                        .await
                }
            }
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
                    ACTION_INIT => self.handle_cmd_init().await,
                    ACTION_SHOW => self.handle_cmd_show().await,
                    "download" => self.my_handle_cmd_downlad(&cmd_parts).await,
                    _ => {
                        self.warn(
                            MODULE,
                            format!(
                                "[{MODULE}] Unknown action ({action}) for cmd `{}`.",
                                cmd.cmd
                            ),
                        )
                        .await;
                    }
                }
            } else {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}
