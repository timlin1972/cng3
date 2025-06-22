use std::io::Write;

use async_trait::async_trait;
use log::Level::{Info, Warn};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, Sender};
use tokio::time::{Duration, sleep};

use crate::messages::{ACTION_ARROW, ACTION_INIT, Cmd, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils::{self, Mode};

const MODULE: &str = "cli";

fn prompt() {
    print!("{} > ", utils::ts_str(utils::ts()));
    std::io::stdout()
        .flush()
        .map_err(|e| e.to_string())
        .expect("Failed to flush");
}

async fn start_input_loop_cli(msg_tx: mpsc::Sender<Msg>, mut shutdown_rx: broadcast::Receiver<()>) {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    prompt();

    loop {
        tokio::select! {
            maybe_line = lines.next_line() => {
                match maybe_line {
                    Ok(Some(line)) => {
                        let msg = Msg {
                            ts: utils::ts(),
                            module: MODULE.to_string(),
                            data: Data::Cmd(Cmd { cmd: line }),
                        };

                        let _ = msg_tx.send(msg).await;

                        sleep(Duration::from_secs(1)).await;
                        prompt();
                    }
                    Ok(None) => break, // EOF
                    Err(e) => {
                        let msg = Msg {
                            ts: utils::ts(),
                            module: MODULE.to_string(),
                            data: Data::Log(Log { level: log::Level::Warn, msg: format!("[{MODULE}] Failed to read input. Err: {e}") }),
                        };

                        let _ = msg_tx.send(msg).await;
                        break;
                    }
                }
            }

            _ = shutdown_rx.recv() => {
                break;
            }
        }
    }
}

async fn start_input_loop_gui(
    msg_tx: mpsc::Sender<Msg>,
    mut shutdown_rx: broadcast::Receiver<()>,
    gui_panel: String,
) {
    let mut output = String::new();

    // 建立 channel 傳送 key event（spawn_blocking 到 async）
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<KeyEvent>(32);
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    let shutdown_flag_clone = shutdown_flag.clone();

    let input_task = tokio::task::spawn_blocking(move || {
        loop {
            // 非同步 poll，避免卡住
            if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    // 把 key 傳出去給 async task 處理
                    if input_tx.blocking_send(key).is_err() {
                        break;
                    }
                }
            }

            // 用 channel 檢查是否該退出（後面 async 部分會處理這個）
            if shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
        }
    });

    loop {
        tokio::select! {
            Some(key) = input_rx.recv() => {
                if key.modifiers == KeyModifiers::ALT {
                    let action = match key.code {
                        KeyCode::Up => Some("location up"),
                        KeyCode::Down => Some("location down"),
                        KeyCode::Left => Some("location left"),
                        KeyCode::Right => Some("location right"),
                        KeyCode::Char('d') => Some("size +x"),
                        KeyCode::Char('a') => Some("size -x"),
                        KeyCode::Char('s') => Some("size +y"),
                        KeyCode::Char('w') => Some("size -y"),
                        KeyCode::Char('c') => Some("output_clear"),
                        _ => None
                    };
                    if let Some(action) = action {
                        let msg = Msg {
                            ts: utils::ts(),
                            module: MODULE.to_string(),
                            data: Data::Cmd(Cmd { cmd: format!("p panels {action}") }),
                        };
                        let _ = msg_tx.send(msg).await;
                    }
                } else {
                    match key.code {
                        KeyCode::Tab => {
                            let msg = Msg {
                                ts: utils::ts(),
                                module: MODULE.to_string(),
                                data: Data::Cmd(Cmd { cmd: "p panels tab".to_string() }),
                            };
                            let _ = msg_tx.send(msg).await;
                        }
                        KeyCode::Char(c) => {
                            output.push(c);

                            utils::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Backspace => {
                            output.pop();

                            utils::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Enter => {
                            let msg = Msg {
                                ts: utils::ts(),
                                module: MODULE.to_string(),
                                data: Data::Cmd(Cmd { cmd: output.clone() }),
                            };
                            let _ = msg_tx.send(msg).await;

                            output.clear();

                            utils::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Left => {
                            let msg = Msg {
                                ts: utils::ts(),
                                module: MODULE.to_string(),
                                data: Data::Cmd(Cmd { cmd: format!("p panels {ACTION_ARROW} left") }),
                            };
                            let _ = msg_tx.send(msg).await;
                        }
                        KeyCode::Right => {
                            let msg = Msg {
                                ts: utils::ts(),
                                module: MODULE.to_string(),
                                data: Data::Cmd(Cmd { cmd: format!("p panels {ACTION_ARROW} right") }),
                            };
                            let _ = msg_tx.send(msg).await;
                        }
                        _ => {}
                    }
                }
            }

            _ = shutdown_rx.recv() => {
                // 通知 blocking thread 結束
                shutdown_flag_clone.store(true, Ordering::Relaxed);
                break;
            }
        }
    }

    // 等待 blocking thread 結束
    let _ = input_task.await;
}

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    mode: Mode,
    started: bool,
    gui_panel: String,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: log::Level::Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            mode: Mode::ModeGui,
            started: false,
            gui_panel: String::new(),
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
            if let (Some(action), Some(mode)) = (cmd_parts.get(2), cmd_parts.get(3)) {
                match action.as_str() {
                    ACTION_INIT => match mode.as_str() {
                        "gui" => {
                            // avoid re-entry
                            if self.started && self.mode == Mode::ModeGui {
                                self.log(
                                    MODULE,
                                    Warn,
                                    format!("[{MODULE}] Started and GUI mode already. Ignore."),
                                )
                                .await;
                                return;
                            }

                            if let Some(gui_panel) = cmd_parts.get(4) {
                                self.started = true;
                                self.mode = Mode::ModeGui;
                                self.gui_panel = gui_panel.to_string();

                                // update prompt
                                utils::output_update_gui_simple(
                                    MODULE,
                                    &self.msg_tx,
                                    &self.gui_panel,
                                    ">".to_string(),
                                )
                                .await;

                                let shutdown_rx = self.shutdown_tx.subscribe();
                                tokio::spawn(start_input_loop_gui(
                                    self.msg_tx.clone(),
                                    shutdown_rx,
                                    self.gui_panel.clone(),
                                ));

                                self.log(
                                    MODULE,
                                    Info,
                                    format!("[{MODULE}] init gui mode (panel: `{gui_panel}`)"),
                                )
                                .await;
                            }
                        }
                        "cli" => {
                            // avoid re-entry
                            if self.started && self.mode == Mode::ModeCli {
                                self.log(
                                    MODULE,
                                    Warn,
                                    format!("[{MODULE}] Started and CLI mode already. Ignore."),
                                )
                                .await;
                                return;
                            }
                            self.started = true;
                            self.mode = Mode::ModeCli;

                            let shutdown_rx = self.shutdown_tx.subscribe();
                            tokio::spawn(start_input_loop_cli(self.msg_tx.clone(), shutdown_rx));

                            self.log(MODULE, Info, format!("[{MODULE}] init cli mode"))
                                .await;
                        }
                        _ => {
                            self.log(
                                MODULE,
                                log::Level::Warn,
                                format!("[{MODULE}] Unknown mode ({mode}) for cmd `{}`.", cmd.cmd),
                            )
                            .await;
                        }
                    },
                    _ => {
                        self.log(
                            MODULE,
                            log::Level::Warn,
                            format!(
                                "[{MODULE}] Unknown action ({action}) for cmd `{}`.",
                                cmd.cmd
                            ),
                        )
                        .await;
                    }
                }
            } else {
                self.log(
                    MODULE,
                    log::Level::Warn,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}
