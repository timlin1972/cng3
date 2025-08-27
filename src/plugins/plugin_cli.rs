use std::io::Write;
use std::sync::Arc;

use async_trait::async_trait;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::io::{self, AsyncBufReadExt, BufReader};
use tokio::select;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::task;
use tokio::time::{Duration, sleep};

use crate::cfg;
use crate::messages::{ACTION_ARROW, ACTION_GUI, ACTION_INIT, Cmd, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils::{self, mode::Mode, panel};

const MODULE: &str = "cli";

fn prompt() {
    print!("{} > ", utils::time::ts_str(utils::time::ts()));
    std::io::stdout()
        .flush()
        .map_err(|e| e.to_string())
        .expect("Failed to flush");
}

async fn start_input_loop_cli(msg_tx: Sender<Msg>, mut shutdown_rx: broadcast::Receiver<()>) {
    let stdin = io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    prompt();

    loop {
        tokio::select! {
            maybe_line = lines.next_line() => {
                match maybe_line {
                    Ok(Some(line)) => {
                        cmd(&msg_tx, line).await;
                        sleep(Duration::from_secs(1)).await;
                        prompt();
                    }
                    Ok(None) => break, // EOF
                    Err(e) => {
                        let msg = Msg {
                            ts: utils::time::ts(),
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
    output: Arc<Mutex<String>>,
    history: Arc<Mutex<Vec<String>>>,
    history_index: Arc<Mutex<usize>>,
    msg_tx: Sender<Msg>,
    mut shutdown_rx: broadcast::Receiver<()>,
    gui_panel: String,
) {
    // 建立 channel 傳送 key event（spawn_blocking 到 async）
    let (input_tx, mut input_rx) = tokio::sync::mpsc::channel::<KeyEvent>(32);
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let shutdown_flag = Arc::new(AtomicBool::new(false));

    let shutdown_flag_clone = shutdown_flag.clone();

    let input_task = task::spawn_blocking(move || {
        loop {
            // 非同步 poll，避免卡住
            if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
                #[allow(clippy::collapsible_if)]
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
                if key.modifiers == KeyModifiers::CONTROL {
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
                        cmd(&msg_tx, format!("p panels {action}")).await;
                    }
                } else {
                    match key.code {
                        KeyCode::Tab => cmd(&msg_tx, "p panels tab".to_string()).await,
                        KeyCode::Char(c) => {
                            let mut output = output.lock().await;
                            output.push(c);
                            panel::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Backspace => {
                            let mut output = output.lock().await;
                            output.pop();
                            panel::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Enter => {
                            let mut output = output.lock().await;
                            let mut history = history.lock().await;
                            let mut history_index = history_index.lock().await;

                            // ignore if the input is as the same as the last one
                            if history.is_empty()
                                || *history.last().unwrap() != *output
                            {
                                history.push(output.clone());
                                *history_index = history.len();
                            }

                            cmd(&msg_tx, output.clone()).await;
                            output.clear();
                            panel::output_update_gui_simple(MODULE, &msg_tx, &gui_panel, format!("> {output}")).await;
                        }
                        KeyCode::Left => cmd(&msg_tx, format!("p panels {ACTION_ARROW} left")).await,
                        KeyCode::Right => cmd(&msg_tx, format!("p panels {ACTION_ARROW} right")).await,
                        KeyCode::Up => cmd(&msg_tx, format!("p panels {ACTION_ARROW} up")).await,
                        KeyCode::Down => cmd(&msg_tx, format!("p panels {ACTION_ARROW} down")).await,
                        _ => {}
                    }
                }
            }

            _ = shutdown_rx.recv() => {
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
    output: Arc<Mutex<String>>,
    history: Arc<Mutex<Vec<String>>>,
    history_index: Arc<Mutex<usize>>,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            mode: Mode::ModeGui,
            started: false,
            gui_panel: String::new(),
            output: Arc::new(Mutex::new(String::new())),
            history: Arc::new(Mutex::new(vec![])),
            history_index: Arc::new(Mutex::new(0)),
        }
    }

    async fn handle_cmd_arrow(&mut self, cmd_parts: &[String]) {
        if let Some(arrow) = cmd_parts.get(3) {
            match arrow.as_str() {
                "up" => {
                    let mut output = self.output.lock().await;
                    let history = self.history.lock().await;
                    let mut history_index = self.history_index.lock().await;

                    if *history_index > 0 {
                        *history_index -= 1;
                        *output = history[*history_index].clone();
                    }

                    panel::output_update_gui_simple(
                        MODULE,
                        &self.msg_tx,
                        &self.gui_panel,
                        format!("> {output}"),
                    )
                    .await;
                }
                "down" => {
                    let mut output = self.output.lock().await;
                    let history = self.history.lock().await;
                    let mut history_index = self.history_index.lock().await;

                    if *history_index < history.len() {
                        *history_index += 1;
                        if *history_index < history.len() {
                            *output = history[*history_index].clone();
                        } else {
                            output.clear();
                        }
                    }

                    panel::output_update_gui_simple(
                        MODULE,
                        &self.msg_tx,
                        &self.gui_panel,
                        format!("> {output}"),
                    )
                    .await;
                }
                _ => (),
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
            if let (Some(action), Some(mode)) = (cmd_parts.get(2), cmd_parts.get(3)) {
                match action.as_str() {
                    ACTION_INIT => match mode.as_str() {
                        ACTION_GUI => {
                            // avoid re-entry
                            if self.started && self.mode == Mode::ModeGui {
                                self.warn(
                                    MODULE,
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
                                panel::output_update_gui_simple(
                                    MODULE,
                                    &self.msg_tx,
                                    &self.gui_panel,
                                    "> ".to_string(),
                                )
                                .await;

                                let shutdown_rx = self.shutdown_tx.subscribe();
                                let output_clone = Arc::clone(&self.output);
                                let history_clone = Arc::clone(&self.history);
                                let history_index_clone = Arc::clone(&self.history_index);
                                tokio::spawn(start_input_loop_gui(
                                    output_clone,
                                    history_clone,
                                    history_index_clone,
                                    self.msg_tx.clone(),
                                    shutdown_rx,
                                    self.gui_panel.clone(),
                                ));

                                self.info(
                                    MODULE,
                                    format!("[{MODULE}] init gui mode (panel: `{gui_panel}`)"),
                                )
                                .await;

                                // update sub_title
                                let msg_tx_clone = self.msg_tx.clone();
                                let mut shutdown_rx = self.shutdown_tx.subscribe();
                                let gui_panel_clone = self.gui_panel.clone();
                                tokio::spawn(async move {
                                    loop {
                                        select! {
                                            _ = sleep(Duration::from_secs(1)) => {
                                                let ts = utils::time::ts();
                                                let sub_title = format!(" - {} - {}", cfg::name(), utils::time::ts_str(ts));
                                                let msg = Msg {
                                                    ts,
                                                    module: MODULE.to_string(),
                                                    data: Data::Cmd(Cmd { cmd: format!("p panels sub_title {gui_panel_clone} '{sub_title}'") }),
                                                };
                                                let _ = msg_tx_clone.send(msg).await;
                                            }
                                            _ = shutdown_rx.recv() => {
                                                println!("Shutdown signal received. Exiting task.");
                                                break;
                                            }
                                        }
                                    }
                                });
                            }
                        }
                        "cli" => {
                            // avoid re-entry
                            if self.started && self.mode == Mode::ModeCli {
                                self.warn(
                                    MODULE,
                                    format!("[{MODULE}] Started and CLI mode already. Ignore."),
                                )
                                .await;
                                return;
                            }
                            self.started = true;
                            self.mode = Mode::ModeCli;

                            let shutdown_rx = self.shutdown_tx.subscribe();
                            tokio::spawn(start_input_loop_cli(self.msg_tx.clone(), shutdown_rx));

                            self.info(MODULE, format!("[{MODULE}] init cli mode")).await;
                        }
                        _ => {
                            self.warn(
                                MODULE,
                                format!("[{MODULE}] Unknown mode ({mode}) for cmd `{}`.", cmd.cmd),
                            )
                            .await;
                        }
                    },
                    ACTION_ARROW => self.handle_cmd_arrow(&cmd_parts).await,
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

async fn cmd(msg_tx: &Sender<Msg>, cmd: String) {
    let msg = Msg {
        ts: utils::time::ts(),
        module: MODULE.to_string(),
        data: Data::Cmd(Cmd { cmd }),
    };
    let _ = msg_tx.send(msg).await;
}
