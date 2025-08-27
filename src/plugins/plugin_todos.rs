use async_trait::async_trait;
use chrono::{Datelike, Local, NaiveDateTime, TimeZone, Timelike};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;
use tokio::{
    select,
    time::{Duration, sleep},
};
use unicode_width::UnicodeWidthChar;
use uuid::Uuid;

use crate::messages::{ACTION_ADD, ACTION_INIT, ACTION_SHOW, Cmd, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{self, task::Task};

const MODULE: &str = "todos";
const ONCE: &str = "once";
const DAILY: &str = "daily";
const WEEKDAYS: &str = "weekdays";
const THREE_DAYS: u64 = 3;
const CHECK_INTERVAL: u64 = 60;

#[derive(Debug)]
struct TodoTask {
    id: Uuid,
    name: String,
    frequency: String,
    time: String,
    reminder: u32,
}

impl Default for TodoTask {
    fn default() -> Self {
        TodoTask {
            id: Uuid::new_v4(),
            name: String::new(),
            frequency: String::new(),
            time: String::new(),
            reminder: 0,
        }
    }
}

impl TodoTask {
    async fn get_tasks(&mut self) -> Vec<Task> {
        let mut tasks = Vec::new();

        match self.frequency.as_str() {
            ONCE => {
                let scheduled_datetime =
                    NaiveDateTime::parse_from_str(&self.time, "%Y/%m/%d-%H:%M").unwrap();
                let scheduled_timestamp = Local
                    .from_local_datetime(&scheduled_datetime)
                    .unwrap()
                    .timestamp() as u64;

                tasks.push(Task {
                    id: Uuid::new_v4(),
                    parent: self.id,
                    name: self.name.clone(),
                    time: scheduled_timestamp,
                    reminder: self.reminder,
                    done: false,
                    reminded: false,
                    dued: false,
                });
            }
            DAILY => {
                if let Ok(parsed_time) = chrono::NaiveTime::parse_from_str(&self.time, "%H:%M") {
                    let now = Local::now();
                    for day_offset in 0..THREE_DAYS {
                        if let Some(scheduled_datetime) = Local
                            .with_ymd_and_hms(
                                now.year(),
                                now.month(),
                                now.day(),
                                parsed_time.hour(),
                                parsed_time.minute(),
                                0,
                            )
                            .unwrap()
                            .checked_add_signed(chrono::Duration::days(day_offset as i64))
                        {
                            let scheduled_timestamp = scheduled_datetime.timestamp() as u64;
                            tasks.push(Task {
                                id: Uuid::new_v4(),
                                parent: self.id,
                                name: self.name.clone(),
                                time: scheduled_timestamp,
                                reminder: self.reminder,
                                done: false,
                                reminded: false,
                                dued: false,
                            });
                        }
                    }
                }
            }
            WEEKDAYS => (),
            // WEEKDAYS => {,
            //         if let Ok(parsed_time) = chrono::NaiveTime::parse_from_str(&self.time, "%H:%M") {
            //             let now = chrono::Local::now().naive_local();
            //             for day_offset in 0..7 {
            //                 if let Some(scheduled_date) =
            //                     chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), now.day())
            //                         .and_then(|d| d.checked_add_signed(chrono::Duration::days(day_offset)))
            //                 {
            //                     if matches!(
            //                         scheduled_date.weekday(),
            //                         chrono::Weekday::Mon
            //                             | chrono::Weekday::Tue
            //                             | chrono::Weekday::Wed
            //                             | chrono::Weekday::Thu
            //                             | chrono::Weekday::Fri
            //                     ) {
            //                         let scheduled_datetime =
            //                             scheduled_date.and_time(parsed_time);
            //                         if let Some(scheduled_datetime) = scheduled_datetime {
            //                             let scheduled_timestamp = scheduled_datetime.timestamp() as u64;
            //                             tasks.push(Task {
            //                                 id: self.id,
            //                                 name: self.name.clone(),
            //                                 time: scheduled_timestamp,
            //                                 reminder: self.reminder,
            //                                 done: false,
            //                             });
            //                         }
            //                     }
            //                 }
            //             }
            //         }
            //     }
            _ => (),
        }

        tasks
    }
}

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    todos: Vec<TodoTask>,
    tasks: Vec<Task>,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            todos: Vec::new(),
            tasks: Vec::new(),
        }
    }

    async fn start_todo(&mut self, mut shutdown_rx: broadcast::Receiver<()>) {
        let msg_tx_clone = self.msg_tx.clone();
        tokio::spawn(async move {
            // do it first time
            let msg = Msg {
                ts: utils::time::ts(),
                module: MODULE.to_string(),
                data: Data::Cmd(Cmd {
                    cmd: format!("p {MODULE} check"),
                }),
            };
            let _ = &msg_tx_clone.send(msg).await;

            loop {
                select! {
                    _ = sleep(Duration::from_secs(CHECK_INTERVAL)) => {
                        let msg = Msg {
                            ts: utils::time::ts(),
                            module: MODULE.to_string(),
                            data: Data::Cmd(Cmd {
                                cmd: format!("p {MODULE} check", ),
                            })
                        };
                        let _ = &msg_tx_clone.send(msg).await;
                    }
                    _ = shutdown_rx.recv() => {
                        println!("Shutdown signal received. Exiting task.");
                        break;
                    }
                }
            }
        });

        self.info(MODULE, format!("[{MODULE}] Check routine started"))
            .await;
    }

    async fn handle_cmd_init(&mut self, shutdown_rx: broadcast::Receiver<()>) {
        self.start_todo(shutdown_rx).await;
        self.info(MODULE, format!("[{MODULE}] init")).await;
    }

    async fn update_infos(&self, task: &Task) {
        // update infos
        self.cmd(
            MODULE,
            format!(
                "p infos {MODULE} update {} {} {} {} {} {} {} {}",
                task.id,
                task.parent,
                task.name,
                task.time,
                task.done,
                task.dued,
                task.reminder,
                task.reminded
            ),
        )
        .await;
    }

    async fn handle_cmd_add(&mut self, cmd_parts: &[String]) {
        let mut todo_task = TodoTask::default();

        // task
        if let Some(item) = cmd_parts.get(3) {
            if item != "task" {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] No `task` (got `{item}`) for add."),
                )
                .await;
                return;
            }
            if let Some(name) = cmd_parts.get(4) {
                todo_task.name = name.clone();
            } else {
                self.warn(MODULE, format!("[{MODULE}] No `task name` for add."))
                    .await;
            }
        } else {
            self.warn(MODULE, format!("[{MODULE}] No `task` for add."))
                .await;
            return;
        }

        // freq
        if let Some(item) = cmd_parts.get(5) {
            match item.as_str() {
                ONCE => todo_task.frequency = ONCE.into(),
                DAILY => todo_task.frequency = DAILY.into(),
                WEEKDAYS => todo_task.frequency = WEEKDAYS.into(),
                _ => {
                    self.warn(
                        MODULE,
                        format!("[{MODULE}] Invalid frequency (got `{item}`) for add."),
                    )
                    .await;
                    return;
                }
            }
            if let Some(time) = cmd_parts.get(6) {
                todo_task.time = time.clone();
            } else {
                self.warn(MODULE, format!("[{MODULE}] No `task time` for add."))
                    .await;
            }
        } else {
            self.warn(
                MODULE,
                format!("[{MODULE}] No `{ONCE}/{DAILY}/{WEEKDAYS}` for add."),
            )
            .await;
            return;
        }

        // reminder
        if let Some(item) = cmd_parts.get(7) {
            if item != "reminder" {
                self.warn(
                    MODULE,
                    format!("[{MODULE}] No `reminder` (got `{item}`) for add."),
                )
                .await;
                return;
            }
            if let Some(reminder) = cmd_parts.get(8) {
                todo_task.reminder = reminder.parse().unwrap_or(0);
            } else {
                self.warn(MODULE, format!("[{MODULE}] No `task reminder` for add."))
                    .await;
            }
        } else {
            self.warn(MODULE, format!("[{MODULE}] No `task reminder` for add."))
                .await;
        }

        todo_task.id = Uuid::new_v4();

        self.info(
            MODULE,
            format!(
                "[{MODULE}] Add: {} {} {}, reminder {} mins",
                todo_task.name, todo_task.frequency, todo_task.time, todo_task.reminder
            ),
        )
        .await;

        let tasks = todo_task.get_tasks().await;
        for task in &tasks {
            self.update_infos(task).await;
        }

        self.todos.push(todo_task);
        self.tasks.extend(tasks);
        self.tasks.sort_by_key(|e| e.time);
    }

    async fn handle_cmd_show(&mut self) {
        self.info(
            MODULE,
            format!(
                "{:<3} {:<12} {:<7} {:<16} {:<10}",
                "Idx", "Name", "Freq", "Time", "Reminder"
            ),
        )
        .await;
        for (idx, todo) in self.todos.iter().enumerate() {
            let name_width: usize = todo.name.chars().map(|c| c.width().unwrap_or(0)).sum();
            let name_space = " ".repeat(12 - name_width);

            self.info(
                MODULE,
                format!(
                    "{idx:<3} {}{} {:<7} {:<16} {:<10}",
                    todo.name, name_space, todo.frequency, todo.time, todo.reminder
                ),
            )
            .await;
        }

        self.tasks.sort_by_key(|e| e.time);

        self.info(
            MODULE,
            format!(
                "{:<3} {:4} {:4} {:4} {:<12} {:<16} {:<10}",
                "Idx", "Done", "Rem", "Due", "Name", "Time", "Reminder"
            ),
        )
        .await;
        for (idx, task) in self.tasks.iter().enumerate() {
            let name_width: usize = task.name.chars().map(|c| c.width().unwrap_or(0)).sum();
            let name_space = " ".repeat(12 - name_width);
            let done_str = if task.done { "✓" } else { "✗" };
            let reminder_str = if task.reminded { "✓" } else { "✗" };
            let dued_str = if task.dued { "✓" } else { "✗" };

            self.info(
                MODULE,
                format!(
                    "{idx:<3} {done_str:4} {reminder_str:4} {dued_str:4} {}{} {:<16} {:<10}",
                    task.name,
                    name_space,
                    utils::time::ts_str_no_tz_no_sec(task.time),
                    task.reminder
                ),
            )
            .await;
        }
    }

    async fn handle_cmd_done(&mut self, cmd_parts: &[String], done: bool) {
        let mut updated = Vec::new();

        if let Some(index) = cmd_parts.get(3) {
            if let Ok(index) = index.parse::<usize>() {
                let mut name = String::new();
                if let Some(task) = self.tasks.get_mut(index) {
                    task.done = done;
                    name = task.name.clone();
                    updated.push(task.id);
                } else {
                    self.warn(MODULE, format!("[{MODULE}] Task not found: {index}"))
                        .await;
                }
                if !name.is_empty() {
                    self.info(MODULE, format!("[{MODULE}] Marked task as done: {}", name))
                        .await;
                }
            } else {
                self.warn(MODULE, format!("[{MODULE}] Invalid task index: {index}"))
                    .await;
            }
        } else {
            self.warn(MODULE, format!("[{MODULE}] No task index provided."))
                .await;
        }

        for updated_info in updated {
            if let Some(task) = self.tasks.iter().find(|t| t.id == updated_info) {
                self.update_infos(task).await;
            }
        }
    }

    async fn handle_cmd_check(&mut self) {
        let mut infos = Vec::new();
        let mut updated = Vec::new();

        let now_ts = utils::time::ts();
        for task in self.tasks.iter_mut() {
            // check due always
            if task.time <= now_ts {
                task.dued = true;
                updated.push(task.id);
            }

            if task.done {
                continue;
            }
            if !task.reminded && task.time - task.reminder as u64 * 60 <= now_ts {
                task.reminded = true;
                updated.push(task.id);

                infos.push(format!(
                    "[{MODULE}] Task reminder: {} {}",
                    task.name,
                    utils::time::ts_str_no_tz_no_sec(task.time)
                ));
            }
            if task.time <= now_ts {
                infos.push(format!(
                    "[{MODULE}] Task due: {} {}",
                    task.name,
                    utils::time::ts_str_no_tz_no_sec(task.time)
                ));
            }
        }

        for updated_info in updated {
            if let Some(task) = self.tasks.iter().find(|t| t.id == updated_info) {
                self.update_infos(task).await;
            }
        }

        for info in infos {
            self.info(MODULE, info).await;
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
                    ACTION_INIT => {
                        let shutdown_rx = self.shutdown_tx.subscribe();
                        self.handle_cmd_init(shutdown_rx).await;
                    }
                    ACTION_ADD => self.handle_cmd_add(&cmd_parts).await,
                    ACTION_SHOW => self.handle_cmd_show().await,
                    "done" => self.handle_cmd_done(&cmd_parts, true).await,
                    "undone" => self.handle_cmd_done(&cmd_parts, false).await,
                    "check" => self.handle_cmd_check().await,
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
