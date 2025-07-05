use std::path::Path;
use std::thread;
use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use log::Level::Info;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::{
    sync::{
        Mutex, broadcast,
        mpsc::{self, Sender},
        oneshot,
    },
    task,
    time::{Duration, sleep},
};

use crate::consts::NAS_FOLDER;
use crate::messages::{ACTION_FILE_MODIFY, ACTION_FILE_REMOVE, ACTION_INIT, Cmd, Data, Log, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils;

const MODULE: &str = "monitor";
const DEBOUNCE_DELAY: u64 = 10; // seconds

type DebounceMap = Arc<Mutex<HashMap<(String, EventKind), tokio::task::JoinHandle<()>>>>;

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    inited: bool,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        utils::log::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            inited: false,
        }
    }

    async fn handle_cmd_init(&mut self, mut shutdown_rx: broadcast::Receiver<()>) {
        if self.inited {
            return;
        }
        self.inited = true;

        let msg_tx_clone = self.msg_tx.clone();
        tokio::spawn(async move {
            let debounce_map: DebounceMap = Arc::new(Mutex::new(HashMap::new()));
            let path_to_watch = Path::new(NAS_FOLDER);
            let (tx, mut rx) = mpsc::channel(1024);

            // 用 oneshot 通知 blocking thread 結束
            let (shutdown_blocking_tx, mut shutdown_blocking_rx) = oneshot::channel::<()>();

            let _watcher_handle = task::spawn_blocking(move || {
                let mut watcher = RecommendedWatcher::new(
                    move |res| {
                        if let Ok(event) = res {
                            let _ = tx.blocking_send(event);
                        }
                    },
                    Config::default(),
                )
                .expect("Watcher 初始化失敗");

                watcher
                    .watch(Path::new(path_to_watch), RecursiveMode::Recursive)
                    .expect("無法監聽目錄");

                // blocking thread 等待 shutdown signal，或每秒檢查一次
                loop {
                    if shutdown_blocking_rx.try_recv().is_ok() {
                        break;
                    }
                    thread::sleep(Duration::from_secs(1));
                }
            });

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        for path in &event.paths {
                            let path_str = path.display().to_string();
                            let debounce_map = debounce_map.clone();

                            let key = (path_str.clone(), event.kind);

                            // cancel the previous task if it exists
                            let mut map = debounce_map.lock().await;
                            if let Some(handle) = map.remove(&key) {
                                handle.abort(); // Abort the previous task
                            }

                            let event_clone = event.clone();
                            let msg_tx_clone_clone = msg_tx_clone.clone();

                            // spawn a new task with a debounce delay
                            let handle = tokio::spawn(async move {
                                sleep(Duration::from_secs(DEBOUNCE_DELAY)).await;
                                handle_event(event_clone, &msg_tx_clone_clone).await;
                            });

                            // store the new task handle in the map
                            map.insert(key, handle);
                        }
                    }

                    _ = shutdown_rx.recv() => {
                        let _ = shutdown_blocking_tx.send(());
                        break;
                    }
                }
            }
        });

        self.info(MODULE, format!("[{MODULE}] init")).await;
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
                    ACTION_INIT => {
                        let shutdown_rx = self.shutdown_tx.subscribe();
                        self.handle_cmd_init(shutdown_rx).await;
                    }
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

fn monitor_get_file(file_path: &str) -> String {
    let keyword = format!("{NAS_FOLDER}/");
    if let Some(pos) = file_path.find(&keyword) {
        let result = &file_path[pos..];
        return result.to_owned();
    }

    "".to_owned()
}

async fn handle_event(event: Event, msg_tx: &Sender<Msg>) {
    match event.kind {
        notify::event::EventKind::Create(_) => (),
        notify::event::EventKind::Modify(_) => {
            for path in event.paths.iter() {
                let filename = monitor_get_file(path.to_str().unwrap());

                let msg = Msg {
                    ts: utils::time::ts(),
                    module: MODULE.to_string(),
                    data: Data::Log(Log {
                        level: Info,
                        msg: format!("[{MODULE}] File is modified: {filename}"),
                    }),
                };
                let _ = msg_tx.send(msg).await;

                let msg = Msg {
                    ts: utils::time::ts(),
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!(
                            "p nas {ACTION_FILE_MODIFY} {}",
                            general_purpose::STANDARD.encode(filename)
                        ),
                    }),
                };
                let _ = msg_tx.send(msg).await;
            }
        }
        notify::event::EventKind::Remove(_) => {
            for path in event.paths.iter() {
                let filename = monitor_get_file(path.to_str().unwrap());

                let msg = Msg {
                    ts: utils::time::ts(),
                    module: MODULE.to_string(),
                    data: Data::Log(Log {
                        level: Info,
                        msg: format!("[{MODULE}] File is removed: {filename}"),
                    }),
                };
                let _ = msg_tx.send(msg).await;

                let msg = Msg {
                    ts: utils::time::ts(),
                    module: MODULE.to_string(),
                    data: Data::Cmd(Cmd {
                        cmd: format!(
                            "p nas {ACTION_FILE_REMOVE} {}",
                            general_purpose::STANDARD.encode(filename)
                        ),
                    }),
                };
                let _ = msg_tx.send(msg).await;
            }
        }
        notify::event::EventKind::Access(_) => (),
        _ => (),
    }
}
