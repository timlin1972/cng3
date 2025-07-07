use async_trait::async_trait;
use chrono::{Datelike, NaiveDate};
use tokio::sync::mpsc::Sender;
use unicode_width::UnicodeWidthChar;

use crate::cfg;
use crate::messages::{
    ACTION_APP_UPTIME, ACTION_ARROW, ACTION_DEVICES, ACTION_NAS_STATE, ACTION_ONBOARD, ACTION_SHOW,
    ACTION_TAILSCALE_IP, ACTION_TEMPERATURE, ACTION_VERSION, Cmd, Data, Msg,
};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{
    self,
    dev_info::{self, DevInfo},
    nas_info::{NasInfo, NasState},
    panel,
    weather::{self, City, Weather, WeatherDaily},
};

const MODULE: &str = "infos";
const PAGES: u16 = 4;

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    gui_panel: String,
    devices: Vec<DevInfo>,
    nas_server: String,
    nas_state: NasState,     // For client
    nas_infos: Vec<NasInfo>, // For server
    page_idx: u16,
    cities: Vec<City>,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            gui_panel: String::new(),
            devices: vec![],
            nas_server: String::new(),
            nas_state: NasState::Unsync,
            nas_infos: vec![],
            page_idx: 0,
            cities: vec![],
        }
    }

    async fn panel_output_update(&mut self) {
        // update sub_title
        let sub_title = format!(" - {}/{PAGES}", self.page_idx + 1);
        let msg = Msg {
            ts: utils::time::ts(),
            module: MODULE.to_string(),
            data: Data::Cmd(Cmd {
                cmd: format!("p panels sub_title {} '{sub_title}'", self.gui_panel),
            }),
        };
        let _ = self.msg_tx.send(msg).await;

        let mut output = String::new();
        match self.page_idx {
            0 => {
                output = format!(
                    "{:<12} {:<7} {:<10} {:16} {:<7} {:13}",
                    "Name", "Onboard", "Version", "Tailscale IP", "Temper", "App uptime"
                );
                for device in &self.devices {
                    output += &format!(
                        "\n{:<12} {:<7} {:<10} {:16} {:<7} {:13}",
                        device.name,
                        dev_info::onboard_str(device.onboard),
                        device.version.clone().unwrap_or("n/a".to_string()),
                        device.tailscale_ip.clone().unwrap_or("n/a".to_string()),
                        dev_info::temperature_str(device.temperature),
                        dev_info::app_uptime_str(device.app_uptime),
                    );
                }
            }
            1 => match self.nas_server == cfg::name() {
                true => {
                    output = format!("{:<12} {:<7} {:10}", "Name", "Onboard", "NAS State");
                    for nas_info in &self.nas_infos {
                        output += &format!(
                            "\n{:<12} {:<7} {:10?}",
                            nas_info.name,
                            dev_info::onboard_str(nas_info.onboard),
                            nas_info.nas_state
                        );
                    }
                }
                false => {
                    output = format!("Nas State: {:?}", self.nas_state);
                }
            },
            2 => {
                output = format!(
                    "{:<12} {:<11} {:7} {:20}",
                    "City", "Update", "Temper", "Weather"
                );
                for city in &self.cities {
                    let (update, temperature, weather) = match &city.weather {
                        Some(weather) => (
                            utils::time::ts_str(
                                utils::time::datetime_str_to_ts(&weather.time) as u64
                            ),
                            format!("{:.1}°C", weather.temperature),
                            weather::weather_code_str(weather.weathercode).to_owned(),
                        ),
                        None => ("n/a".to_owned(), "n/a".to_owned(), "n/a".to_owned()),
                    };

                    let name_width: usize = city.name.chars().map(|c| c.width().unwrap_or(0)).sum();
                    let name_space = " ".repeat(12 - name_width);

                    output += &format!(
                        "\n{}{name_space} {update:<11} {temperature:7} {weather:20}",
                        city.name
                    );
                }
            }
            3 => {
                if self.cities.is_empty() {
                    return;
                }
                if self.cities[0].weather.is_none() {
                    return;
                }

                let weather = self.cities[0].weather.as_ref().unwrap();
                output.push_str(&format!("{:<12} ", "City"));
                for (idx, daily) in weather.daily.iter().enumerate() {
                    if idx == 0 {
                        continue;
                    }
                    output.push_str(&format!("{:<27} ", format_date(&daily.time)));
                }

                for city in &self.cities {
                    let name_width: usize = city.name.chars().map(|c| c.width().unwrap_or(0)).sum();
                    let name_space = " ".repeat(12 - name_width);

                    output.push_str(&format!("\n{}{name_space} ", city.name));
                    if let Some(weather) = &city.weather {
                        for (idx, daily) in weather.daily.iter().enumerate() {
                            if idx == 0 {
                                continue;
                            }
                            let (
                                temperature,
                                precipitation_probability_max,
                                weather_emoji,
                                weather,
                            ) = (
                                format!(
                                    "{:.0}/{:.0}",
                                    daily.temperature_2m_max, daily.temperature_2m_min
                                ),
                                format!("{}%", daily.precipitation_probability_max),
                                weather::weather_code_emoji(daily.weather_code).to_owned(),
                                weather::weather_code_str(daily.weather_code).to_owned(),
                            );
                            output.push_str(&format!(
                                "{weather_emoji} {precipitation_probability_max:4} {temperature:6} "
                            ));
                            output.push_str(&weather);
                            output.push_str(" ".repeat(13 - weather.len() * 2 / 3).as_str());
                        }
                    }
                }
            }
            _ => (),
        }

        panel::output_update_gui_simple(MODULE, &self.msg_tx, &self.gui_panel, output).await;
    }

    async fn handle_cmd_devices(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            let ts = utils::time::ts();
            match action.as_str() {
                ACTION_ONBOARD => {
                    if let (Some(name), Some(onboard)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let onboard = onboard == "1";
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.onboard = onboard;
                        } else {
                            let device_add = DevInfo {
                                ts,
                                name: name.to_string(),
                                onboard,
                                version: None,
                                tailscale_ip: None,
                                temperature: None,
                                app_uptime: None,
                            };
                            self.devices.push(device_add.clone());
                        }
                    }
                }
                ACTION_VERSION => {
                    if let (Some(name), Some(version)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.version = Some(version.to_string());
                        }
                    }
                }
                ACTION_TAILSCALE_IP => {
                    if let (Some(name), Some(tailscale_ip)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.tailscale_ip = Some(tailscale_ip.to_string());
                        }
                    }
                }
                ACTION_TEMPERATURE => {
                    if let (Some(name), Some(temperature)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.temperature = Some(temperature.parse::<f32>().unwrap());
                        }
                    }
                }
                ACTION_APP_UPTIME => {
                    if let (Some(name), Some(app_uptime)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        if let Some(device) =
                            self.devices.iter_mut().find(|device| device.name == *name)
                        {
                            device.ts = ts;
                            device.app_uptime = Some(app_uptime.parse::<u64>().unwrap());
                        }
                    }
                }
                _ => (),
            }
            self.panel_output_update().await;
        }
    }

    async fn handle_cmd_nas(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            let ts = utils::time::ts();
            match action.as_str() {
                ACTION_ONBOARD => {
                    if let (Some(name), Some(onboard)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let onboard = onboard == "1";
                        if let Some(nas_info) = self
                            .nas_infos
                            .iter_mut()
                            .find(|nas_info| nas_info.name == *name)
                        {
                            nas_info.ts = ts;
                            nas_info.onboard = onboard;
                        } else {
                            let nas_info_add = NasInfo {
                                ts,
                                name: name.to_string(),
                                onboard,
                                nas_state: NasState::Unsync,
                                tailscale_ip: None,
                            };
                            self.nas_infos.push(nas_info_add.clone());
                        }
                    }
                }
                "nas_server" => {
                    if let Some(nas_server) = cmd_parts.get(4) {
                        self.nas_server = nas_server.clone();
                    }
                }
                ACTION_NAS_STATE => {
                    // server
                    if let (Some(name), Some(nas_state)) = (cmd_parts.get(4), cmd_parts.get(5)) {
                        let nas_state = match nas_state.as_str() {
                            "Unsync" => NasState::Unsync,
                            "Syncing" => NasState::Syncing,
                            "Synced" => NasState::Synced,
                            "Err" => NasState::Err,
                            _ => todo!(),
                        };

                        if let Some(nas_info) = self
                            .nas_infos
                            .iter_mut()
                            .find(|nas_info| nas_info.name == *name)
                        {
                            nas_info.ts = ts;
                            nas_info.nas_state = nas_state;
                        }
                    }
                    // client
                    else if let Some(nas_state) = cmd_parts.get(4) {
                        match nas_state.as_str() {
                            "Unsync" => self.nas_state = NasState::Unsync,
                            "Syncing" => self.nas_state = NasState::Syncing,
                            "Synced" => self.nas_state = NasState::Synced,
                            "Err" => self.nas_state = NasState::Err,
                            _ => todo!(),
                        }
                    } else {
                        self.warn(
                            MODULE,
                            format!("[{MODULE}] Missing {ACTION_NAS_STATE} or name/{ACTION_NAS_STATE} for cmd `{cmd_parts:?}`."),
                        )
                        .await;
                    }
                }
                _ => {
                    self.warn(
                        MODULE,
                        format!("[{MODULE}] Unknown action ({action}) for cmd `{cmd_parts:?}`."),
                    )
                    .await
                }
            }
            self.panel_output_update().await;
        }
    }

    async fn handle_cmd_show(&mut self) {
        self.info(
            MODULE,
            format!(
                "{:<12} {:<7} {:16} {:<13}",
                "Name", "Onboard", "Tailscale IP", "App uptime"
            ),
        )
        .await;
        for device in &self.devices {
            self.info(
                MODULE,
                format!(
                    "{:<12} {:<7} {:16} {:<13}",
                    device.name,
                    dev_info::onboard_str(device.onboard),
                    device.tailscale_ip.clone().unwrap_or("n/a".to_string()),
                    dev_info::app_uptime_str(device.app_uptime)
                ),
            )
            .await;
        }

        self.info(
            MODULE,
            format!("{:<12} {:<7} {:10}", "Name", "Onboard", "NAS State"),
        )
        .await;
        for nas_info in &self.nas_infos {
            self.info(
                MODULE,
                format!(
                    "{:<12} {:<7} {:10?}",
                    nas_info.name,
                    dev_info::onboard_str(nas_info.onboard),
                    nas_info.nas_state
                ),
            )
            .await;
        }
    }

    async fn handle_cmd_weather(&mut self, cmd_parts: &[String]) {
        if let Some(action) = cmd_parts.get(3) {
            match action.as_str() {
                "add" => {
                    if let (Some(name), Some(latitude), Some(longitude)) =
                        (cmd_parts.get(4), cmd_parts.get(5), cmd_parts.get(6))
                    {
                        if !self.cities.iter().any(|city| city.name == *name) {
                            self.cities.push(City {
                                name: name.to_string(),
                                latitude: latitude.parse::<f32>().unwrap(),
                                longitude: longitude.parse::<f32>().unwrap(),
                                weather: None,
                            });
                        }
                    }
                }
                "update_item" => {
                    if let Some(class) = cmd_parts.get(4) {
                        match class.as_str() {
                            "summary" => {
                                if let (
                                    Some(name),
                                    Some(time),
                                    Some(temperature),
                                    Some(weathercode),
                                ) = (
                                    cmd_parts.get(5),
                                    cmd_parts.get(6),
                                    cmd_parts.get(7),
                                    cmd_parts.get(8),
                                ) {
                                    if let Some(city) =
                                        self.cities.iter_mut().find(|city| city.name == *name)
                                    {
                                        let time = time.to_string();
                                        let temperature = temperature.parse::<f32>().unwrap();
                                        let weathercode = weathercode.parse::<u8>().unwrap();

                                        if let Some(weather) = city.weather.as_mut() {
                                            weather.time = time;
                                            weather.temperature = temperature;
                                            weather.weathercode = weathercode;
                                        } else {
                                            city.weather = Some(Weather {
                                                time,
                                                temperature,
                                                weathercode,
                                                daily: vec![],
                                            });
                                        }
                                    }
                                }
                            }
                            "daily" => {
                                if let (
                                    Some(name),
                                    Some(idx),
                                    Some(time),
                                    Some(temperature_2m_max),
                                    Some(temperature_2m_min),
                                    Some(precipitation_probability_max),
                                    Some(weather_code),
                                ) = (
                                    cmd_parts.get(5),
                                    cmd_parts.get(6),
                                    cmd_parts.get(7),
                                    cmd_parts.get(8),
                                    cmd_parts.get(9),
                                    cmd_parts.get(10),
                                    cmd_parts.get(11),
                                ) {
                                    if let Some(city) =
                                        self.cities.iter_mut().find(|city| city.name == *name)
                                    {
                                        let idx = idx.parse::<usize>().unwrap();
                                        let daily = WeatherDaily {
                                            time: time.to_string(),
                                            temperature_2m_max: temperature_2m_max
                                                .parse::<f32>()
                                                .unwrap(),
                                            temperature_2m_min: temperature_2m_min
                                                .parse::<f32>()
                                                .unwrap(),
                                            precipitation_probability_max:
                                                precipitation_probability_max.parse::<u8>().unwrap(),
                                            weather_code: weather_code.parse::<u8>().unwrap(),
                                        };

                                        if let Some(weather) = city.weather.as_mut() {
                                            if weather.daily.len() <= idx {
                                                weather.daily.resize_with(idx + 1, || {
                                                    WeatherDaily {
                                                        time: String::new(),
                                                        temperature_2m_max: 0.0,
                                                        temperature_2m_min: 0.0,
                                                        precipitation_probability_max: 0,
                                                        weather_code: 0,
                                                    }
                                                });
                                            }

                                            weather.daily[idx] = daily;
                                        }
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                }
                _ => {
                    self.warn(
                        MODULE,
                        format!("[{MODULE}] Unknown action ({action}) for cmd `{cmd_parts:?}`.",),
                    )
                    .await;
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
                    "gui" => {
                        if let Some(gui_panel) = cmd_parts.get(3) {
                            self.gui_panel = gui_panel.to_string();
                        }
                    }
                    ACTION_SHOW => self.handle_cmd_show().await,
                    ACTION_DEVICES => self.handle_cmd_devices(&cmd_parts).await,
                    "nas" => self.handle_cmd_nas(&cmd_parts).await,
                    ACTION_ARROW => {
                        if let Some(left_right) = cmd_parts.get(3) {
                            match left_right.as_str() {
                                "right" => self.page_idx = (self.page_idx + 1) % PAGES,
                                "left" => self.page_idx = (self.page_idx + PAGES - 1) % PAGES,
                                _ => (),
                            }
                        }

                        self.panel_output_update().await;
                    }
                    "weather" => self.handle_cmd_weather(&cmd_parts).await,
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

fn format_date(input: &str) -> String {
    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d").expect("無法解析日期");
    format!("{} {}", date.format("%m/%d"), date.weekday())
}
