use std::vec;

use async_trait::async_trait;
use log::Level::Info;
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Style},
    text::Text,
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_CREATE, ACTION_INIT, ACTION_SHOW, Data, Log, Msg};
use crate::plugins::plugins_main;
use crate::utils;

const MODULE: &str = "panels";

#[derive(Debug)]
struct Panel {
    title: String,
    x: u16,
    y: u16,
    x_width: u16,
    y_height: u16,
    output: Vec<String>,
}

#[derive(Debug)]
pub struct Plugin {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    inited: bool,
    terminal: Option<DefaultTerminal>,
    active_panel: usize,
    panels: Vec<Panel>,
}

impl Plugin {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        let msg = Msg {
            ts: utils::ts(),
            module: MODULE.to_string(),
            data: Data::Log(Log {
                level: Info,
                msg: format!("[{MODULE}] new"),
            }),
        };
        msg_tx.send(msg).await.expect("Failed to send message");

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            inited: false,
            terminal: None,
            active_panel: 0,
            panels: vec![],
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        for (idx, panel) in self.panels.iter().enumerate() {
            if idx != self.active_panel {
                draw_panel(panel, frame, false);
            }
        }

        for (idx, panel) in self.panels.iter().enumerate() {
            if idx == self.active_panel {
                draw_panel(panel, frame, true);
                break;
            }
        }
    }

    fn handle_cmd_init(&mut self) {
        if self.inited {
            return;
        }
        self.inited = true;
        self.terminal = Some(ratatui::init());
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;
            ratatui::restore();
        });
    }

    fn handle_cmd_tab(&mut self) {
        if let Some(mut terminal) = self.terminal.take() {
            self.active_panel = (self.active_panel + 1) % self.panels.len();

            let _ = terminal.draw(|frame| self.draw(frame));
            self.terminal = Some(terminal);
        }
    }

    fn handle_cmd_size(&mut self, cmd_parts: &[String]) {
        if let Some(mut terminal) = self.terminal.take() {
            if let Some(action) = cmd_parts.get(3) {
                for (idx, panel) in self.panels.iter_mut().enumerate() {
                    if idx == self.active_panel {
                        match action.as_str() {
                            "+x" => {
                                panel.x_width += 1;
                            }
                            "-x" => {
                                if panel.x_width > 2 {
                                    panel.x_width -= 1;
                                }
                            }
                            "+y" => {
                                panel.y_height += 1;
                            }
                            "-y" => {
                                if panel.y_height > 2 {
                                    panel.y_height -= 1;
                                }
                            }
                            _ => (),
                        }
                        break;
                    }
                }
                let _ = terminal.draw(|frame| self.draw(frame));
                self.terminal = Some(terminal);
            }
        }
    }

    fn handle_cmd_location(&mut self, cmd_parts: &[String]) {
        if let Some(mut terminal) = self.terminal.take() {
            if let Some(direction) = cmd_parts.get(3) {
                for (idx, panel) in self.panels.iter_mut().enumerate() {
                    if idx == self.active_panel {
                        match direction.as_str() {
                            "up" => {
                                if panel.y > 0 {
                                    panel.y -= 1;
                                }
                            }
                            "down" => {
                                panel.y += 1;
                            }
                            "left" => {
                                if panel.x > 0 {
                                    panel.x -= 1;
                                }
                            }
                            "right" => {
                                panel.x += 1;
                            }
                            _ => (),
                        }
                        break;
                    }
                }
                let _ = terminal.draw(|frame| self.draw(frame));
                self.terminal = Some(terminal);
            }
        }
    }
}

#[async_trait]
impl plugins_main::Plugin for Plugin {
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
                        self.handle_cmd_init();
                    }
                    ACTION_SHOW => {
                        self.log(MODULE, Info, format!("[{MODULE}] show")).await;
                        self.log(MODULE, Info, format!("[{MODULE}] inited: {}", self.inited))
                            .await;
                    }
                    "tab" => {
                        self.handle_cmd_tab();
                    }
                    "size" => {
                        self.handle_cmd_size(&cmd_parts);
                    }
                    "location" => {
                        self.handle_cmd_location(&cmd_parts);
                    }
                    "output" => {
                        if let Some(mut terminal) = self.terminal.take() {
                            if let (Some(panel_title), Some(output)) =
                                (cmd_parts.get(3), cmd_parts.get(4))
                            {
                                if let Some(panel) =
                                    self.panels.iter_mut().find(|p| p.title == *panel_title)
                                {
                                    let output_string = output.to_string();
                                    // this is a very special case
                                    if panel_title == "command" {
                                        panel.output.clear();
                                    }
                                    panel.output.push(output_string);
                                }
                                let _ = terminal.draw(|frame| self.draw(frame));
                                self.terminal = Some(terminal);
                            }
                        }
                    }
                    ACTION_CREATE => {
                        if let Some(mut terminal) = self.terminal.take() {
                            if let (Some(title), Some(x), Some(y), Some(x_width), Some(y_height)) = (
                                cmd_parts.get(3),
                                cmd_parts.get(4),
                                cmd_parts.get(5),
                                cmd_parts.get(6),
                                cmd_parts.get(7),
                            ) {
                                let panel = Panel {
                                    title: title.to_string(),
                                    x: x.parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse x (`{x}`)")),
                                    y: y.parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse x (`{x}`)")),
                                    x_width: x_width
                                        .parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse x (`{x}`)")),
                                    y_height: y_height
                                        .parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse x (`{x}`)")),
                                    output: vec![],
                                };
                                self.panels.push(panel);

                                let _ = terminal.draw(|frame| self.draw(frame));
                                self.terminal = Some(terminal);
                            } else {
                                self.log(
                                    MODULE,
                                    Info,
                                    format!(
                                        "[{MODULE}] Missing title/x/y/x_width/y_height for cmd `{}`.",
                                        cmd.cmd
                                    ),
                                )
                                .await;
                            }
                        }
                    }
                    _ => {
                        self.log(
                            MODULE,
                            Info,
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
                    Info,
                    format!("[{MODULE}] Missing action for cmd `{}`.", cmd.cmd),
                )
                .await;
            }
        }
    }
}

fn draw_panel(panel: &Panel, frame: &mut Frame, active: bool) {
    let panel_area = panel_rect(
        panel.x,
        panel.y,
        panel.x_width,
        panel.y_height,
        frame.area(),
    );
    frame.render_widget(Clear, panel_area);

    let panel_block = Block::default()
        .borders(Borders::ALL)
        .title(panel.title.clone())
        .padding(ratatui::widgets::Padding::new(0, 0, 0, 0))
        .border_type(if active {
            BorderType::Double
        } else {
            BorderType::Plain
        })
        .style(Style::default().fg(if active { Color::Cyan } else { Color::White }));

    frame.render_widget(panel_block.clone(), panel_area);

    let area_height = panel_area.height;

    let scroll_offset = if panel.output.len() as u16 > (area_height - 2) {
        panel.output.len() as u16 - (area_height - 2)
    } else {
        0
    };

    let text = Paragraph::new(Text::from(panel.output.join("\n")))
        .style(Style::default().fg(if active { Color::Cyan } else { Color::White }))
        .scroll((scroll_offset, 0));

    frame.render_widget(text, panel_block.inner(panel_area));
}

fn panel_rect(x: u16, y: u16, x_width: u16, y_height: u16, area: Rect) -> Rect {
    let x = area.x.saturating_add(x);
    let y = area.y.saturating_add(y);
    let width = x_width.min(area.width.saturating_sub(x - area.x));
    let height = y_height.min(area.height.saturating_sub(y - area.y));
    Rect {
        x,
        y,
        width,
        height,
    }
}
