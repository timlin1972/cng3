use std::vec;

use async_trait::async_trait;
use log::Level::Info;
use ratatui::{
    DefaultTerminal, Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_ARROW, ACTION_CREATE, ACTION_INIT, ACTION_SHOW, Data, Log, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils;

const MODULE: &str = "panels";

#[derive(Debug)]
struct Panel {
    title: String,
    sub_title: String,
    plugin_name: String,
    x: u16,
    y: u16,
    x_width: u16,
    y_height: u16,
    output: Vec<String>,
}

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    inited: bool,
    terminal: Option<DefaultTerminal>,
    active_panel: usize,
    panels: Vec<Panel>,
}

impl PluginUnit {
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

    async fn handle_cmd_init(&mut self) {
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

        self.info(MODULE, format!("[{MODULE}] init")).await;
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

    async fn handle_cmd_arrow(&mut self, cmd_parts: &[String]) {
        if let Some(arrow) = cmd_parts.get(3) {
            for (idx, panel) in self.panels.iter_mut().enumerate() {
                if idx == self.active_panel {
                    let panel_plugin_name = panel.plugin_name.clone();
                    self.cmd(
                        MODULE,
                        format!("p {panel_plugin_name} {ACTION_ARROW} {arrow}"),
                    )
                    .await;
                    break;
                }
            }
        }
    }

    async fn handle_cmd_sub_title(&mut self, cmd_parts: &[String]) {
        if let Some(mut terminal) = self.terminal.take() {
            if let (Some(panel_title), Some(sub_title)) = (cmd_parts.get(3), cmd_parts.get(4)) {
                if let Some(panel) = self.panels.iter_mut().find(|p| p.title == *panel_title) {
                    panel.sub_title = sub_title.to_string();
                }
            }
            let _ = terminal.draw(|frame| self.draw(frame));
            self.terminal = Some(terminal);
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("[{MODULE}] show")).await;
        self.info(MODULE, format!("[{MODULE}] inited: {}", self.inited))
            .await;
        self.info(
            MODULE,
            format!("{:<12} {:<12} {:12}", "Title", "Subtitle", "Plugin"),
        )
        .await;
        for panel in &self.panels {
            self.info(
                MODULE,
                format!(
                    "{:<12} {:<12} {:12}",
                    panel.title, panel.sub_title, panel.plugin_name
                ),
            )
            .await;
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
                    "tab" => self.handle_cmd_tab(),
                    "size" => self.handle_cmd_size(&cmd_parts),
                    "location" => self.handle_cmd_location(&cmd_parts),
                    ACTION_ARROW => self.handle_cmd_arrow(&cmd_parts).await,
                    "sub_title" => self.handle_cmd_sub_title(&cmd_parts).await,
                    "output_clear" => {
                        if let Some(mut terminal) = self.terminal.take() {
                            for (idx, panel) in self.panels.iter_mut().enumerate() {
                                if idx == self.active_panel {
                                    panel.output.clear();
                                    break;
                                }
                            }
                            let _ = terminal.draw(|frame| self.draw(frame));
                            self.terminal = Some(terminal);
                        }
                    }
                    "output_update" => {
                        if let Some(mut terminal) = self.terminal.take() {
                            if let (Some(panel_title), Some(output)) =
                                (cmd_parts.get(3), cmd_parts.get(4))
                            {
                                if let Some(panel) =
                                    self.panels.iter_mut().find(|p| p.title == *panel_title)
                                {
                                    panel.output.clear();
                                    panel.output.push(output.to_string());
                                }
                            }
                            let _ = terminal.draw(|frame| self.draw(frame));
                            self.terminal = Some(terminal);
                        }
                    }
                    "output_push" => {
                        if let Some(mut terminal) = self.terminal.take() {
                            if let (Some(panel_title), Some(output)) =
                                (cmd_parts.get(3), cmd_parts.get(4))
                            {
                                if let Some(panel) =
                                    self.panels.iter_mut().find(|p| p.title == *panel_title)
                                {
                                    panel.output.push(output.to_string());
                                }
                            }
                            let _ = terminal.draw(|frame| self.draw(frame));
                            self.terminal = Some(terminal);
                        }
                    }
                    ACTION_CREATE => {
                        if let Some(mut terminal) = self.terminal.take() {
                            if let (
                                Some(title),
                                Some(plugin_name),
                                Some(x),
                                Some(y),
                                Some(x_width),
                                Some(y_height),
                            ) = (
                                cmd_parts.get(3),
                                cmd_parts.get(4),
                                cmd_parts.get(5),
                                cmd_parts.get(6),
                                cmd_parts.get(7),
                                cmd_parts.get(8),
                            ) {
                                let panel = Panel {
                                    title: title.to_string(),
                                    sub_title: String::new(),
                                    plugin_name: plugin_name.to_string(),
                                    x: x.parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse x (`{x}`)")),
                                    y: y.parse::<u16>()
                                        .unwrap_or_else(|_| panic!("Failed to parse y (`{y}`)")),
                                    x_width: x_width.parse::<u16>().unwrap_or_else(|_| {
                                        panic!("Failed to parse x_width (`{x_width}`)")
                                    }),
                                    y_height: y_height.parse::<u16>().unwrap_or_else(|_| {
                                        panic!("Failed to parse y_height (`{y_height}`)")
                                    }),
                                    output: vec![],
                                };
                                self.panels.push(panel);

                                let _ = terminal.draw(|frame| self.draw(frame));
                                self.terminal = Some(terminal);
                            } else {
                                self.info(
                                    MODULE,
                                    format!(
                                        "[{MODULE}] Missing title/plugin_name/x/y/x_width/y_height for cmd `{}`.",
                                        cmd.cmd
                                    ),
                                )
                                .await;
                            }
                        }
                    }
                    _ => {
                        self.info(
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
                self.info(
                    MODULE,
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
        .title(format!("{}{}", panel.title, panel.sub_title))
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

    let lines: Vec<Line> = panel
        .output
        .iter()
        .flat_map(|entry| {
            entry
                .split('\n') // 處理內部的換行
                .map(|subline| {
                    if subline.contains("[WARN]") {
                        Line::from(Span::styled(
                            subline.to_string(),
                            Style::default().fg(Color::Red),
                        ))
                    } else {
                        Line::from(Span::raw(subline.to_string()))
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();

    // let text = Paragraph::new(Text::from(panel.output.join("\n")))
    let text = Paragraph::new(Text::from(lines))
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
