use async_trait::async_trait;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{cursor::SetCursorStyle, execute},
    layout::{Position, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_ARROW, ACTION_CREATE, ACTION_INIT, ACTION_SHOW, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils;

const MODULE: &str = "panels";
const MAX_OUTPUT_LEN: usize = 300;
const CURSOR_PANEL_TITLE: &str = "command";

#[derive(Debug)]
struct Panel {
    title: String,
    sub_title: String,
    plugin_name: String,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
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
        utils::msg::log_new(&msg_tx, MODULE).await;

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
        for (idx, panel) in self.panels.iter_mut().enumerate() {
            if idx != self.active_panel {
                draw_panel(panel, frame, false);
            }
        }

        for (idx, panel) in self.panels.iter_mut().enumerate() {
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

        let mut stdout = std::io::stdout();
        execute!(stdout, SetCursorStyle::BlinkingBlock).unwrap();

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        tokio::spawn(async move {
            let _ = shutdown_rx.recv().await;

            let mut stdout = std::io::stdout();
            execute!(stdout, SetCursorStyle::DefaultUserShape).unwrap();

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
                                panel.width += 1;
                            }
                            "-x" => {
                                if panel.width > 2 {
                                    panel.width -= 1;
                                }
                            }
                            "+y" => {
                                panel.height += 1;
                            }
                            "-y" => {
                                if panel.height > 2 {
                                    panel.height -= 1;
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
                                    let panel_output_len = panel.output.len();
                                    if panel_output_len > MAX_OUTPUT_LEN {
                                        panel.output.drain(..panel_output_len - MAX_OUTPUT_LEN);
                                    }
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
                                Some(width),
                                Some(height),
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
                                    width: width.parse::<u16>().unwrap_or_else(|_| {
                                        panic!("Failed to parse width (`{width}`)")
                                    }),
                                    height: height.parse::<u16>().unwrap_or_else(|_| {
                                        panic!("Failed to parse height (`{height}`)")
                                    }),
                                    output: vec![],
                                };
                                self.panels.push(panel);

                                let _ = terminal.draw(|frame| self.draw(frame));
                                self.terminal = Some(terminal);
                            } else {
                                self.warn(
                                    MODULE,
                                    format!(
                                        "[{MODULE}] Missing title/plugin_name/x/y/width/height for cmd `{}`.",
                                        cmd.cmd
                                    ),
                                )
                                .await;
                            }
                        }
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

fn draw_panel(panel: &mut Panel, frame: &mut Frame, active: bool) {
    let width = frame.area().width;
    let height = frame.area().height - 3;

    let (panel_x, panel_y, panel_width, panel_height) = if panel.title == CURSOR_PANEL_TITLE {
        (0, height, width, 3)
    } else {
        (
            (width as f32 * panel.x as f32 / 100.0).round() as u16,
            (height as f32 * panel.y as f32 / 100.0).round() as u16,
            (width as f32 * panel.width as f32 / 100.0).round() as u16,
            (height as f32 * panel.height as f32 / 100.0).round() as u16,
        )
    };

    let panel_area = panel_rect(panel_x, panel_y, panel_width, panel_height, frame.area());
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

    let scroll_offset =
        if panel.title != CURSOR_PANEL_TITLE && panel.output.len() as u16 > (area_height - 3) {
            panel.output.len() as u16 - (area_height - 3)
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

    // cursor is only for panel command
    if panel.title == CURSOR_PANEL_TITLE && !panel.output.is_empty() {
        frame.set_cursor_position(Position::new(
            panel_x + panel.output[0].len() as u16 + 1,
            panel_y + 1,
        ));
    }
}

fn panel_rect(x: u16, y: u16, width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x.saturating_add(x);
    let y = area.y.saturating_add(y);
    let width = width.min(area.width.saturating_sub(x - area.x));
    let height = height.min(area.height.saturating_sub(y - area.y));
    Rect {
        x,
        y,
        width,
        height,
    }
}
