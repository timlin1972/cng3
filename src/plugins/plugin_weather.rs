use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_INIT, ACTION_SHOW, Cmd, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{
    self,
    weather::{self, City, Weather, WeatherDaily},
};

const MODULE: &str = "weather";
const WEATHER_POLLING: u64 = 15 * 60; // 15 mins

#[derive(Debug)]
pub struct PluginUnit {
    name: String,
    msg_tx: Sender<Msg>,
    shutdown_tx: broadcast::Sender<()>,
    inited: bool,
    gui_panel: String,
    cities: Vec<City>,
}

impl PluginUnit {
    pub async fn new(msg_tx: Sender<Msg>, shutdown_tx: broadcast::Sender<()>) -> Self {
        utils::msg::log_new(&msg_tx, MODULE).await;

        Self {
            name: MODULE.to_owned(),
            msg_tx,
            shutdown_tx,
            inited: false,
            gui_panel: "infos".to_string(),
            cities: vec![],
        }
    }

    async fn handle_cmd_update(&mut self) {
        if !self.inited {
            return;
        }

        let cities = self.cities.clone();
        let msg_tx_clone = self.msg_tx.clone();
        let gui_panel_clone = self.gui_panel.clone();
        tokio::spawn(async move {
            for city in &cities {
                let weather = weather::get_weather(city.latitude, city.longitude).await;
                if let Ok(weather) = weather {
                    utils::msg::cmd(
                        &msg_tx_clone,
                        MODULE,
                        format!(
                            "p weather update_item summary {} {} {} {}",
                            city.name, weather.time, weather.temperature, weather.weathercode
                        ),
                    )
                    .await;

                    utils::msg::cmd(
                        &msg_tx_clone,
                        MODULE,
                        format!(
                            "p {gui_panel_clone} weather update_item summary {} {} {} {}",
                            city.name, weather.time, weather.temperature, weather.weathercode
                        ),
                    )
                    .await;

                    for (idx, daily) in weather.daily.iter().enumerate() {
                        utils::msg::cmd(
                            &msg_tx_clone,
                            MODULE,
                            format!(
                                "p weather update_item daily {} {} {} {} {} {} {}",
                                city.name,
                                idx,
                                daily.time,
                                daily.temperature_2m_max,
                                daily.temperature_2m_min,
                                daily.precipitation_probability_max,
                                daily.weather_code,
                            ),
                        )
                        .await;

                        utils::msg::cmd(
                            &msg_tx_clone,
                            MODULE,
                            format!(
                                "p {gui_panel_clone} weather update_item daily {} {} {} {} {} {} {}",
                                city.name,
                                idx,
                                daily.time,
                                daily.temperature_2m_max,
                                daily.temperature_2m_min,
                                daily.precipitation_probability_max,
                                daily.weather_code,
                            )
                        )
                        .await;
                    }
                }
            }
        });
    }

    async fn handle_cmd_init(&mut self) {
        if self.inited {
            return;
        }
        self.inited = true;

        self.cmd(MODULE, "p weather update".to_string()).await;

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let msg_tx_clone = self.msg_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(WEATHER_POLLING)) => {
                        let msg = Msg {
                            ts: utils::time::ts(),
                            module: MODULE.to_string(),
                            data: Data::Cmd(Cmd {
                                cmd: "p weather update".to_string(),
                            }),
                        };

                        let _ = msg_tx_clone.send(msg).await;
                    }
                }
            }
        });

        self.info(MODULE, format!("[{MODULE}] init")).await;
    }

    async fn handle_cmd_show(&mut self) {
        self.info(MODULE, format!("[{MODULE}] Inited: {:?}", self.inited))
            .await;
        self.info(MODULE, format!("{:<12} {:<7}", "Name", "Temp"))
            .await;
        for city in &self.cities {
            let temperature = if let Some(weather) = &city.weather {
                format!("{:.1}°C", weather.temperature)
            } else {
                "n/a".to_string()
            };
            self.info(MODULE, format!("{:<12} {temperature:<7}", city.name,))
                .await;
        }
    }

    async fn handle_cmd_add(&mut self, cmd_parts: &[String]) {
        if let (Some(name), Some(latitude), Some(longitude)) =
            (cmd_parts.get(3), cmd_parts.get(4), cmd_parts.get(5))
        {
            if !self.cities.iter().any(|city| city.name == *name) {
                self.cities.push(City {
                    name: name.to_string(),
                    latitude: latitude.parse::<f32>().unwrap(),
                    longitude: longitude.parse::<f32>().unwrap(),
                    weather: None,
                });

                self.cmd(
                    MODULE,
                    format!(
                        "p {} weather add {name} {latitude} {longitude}",
                        self.gui_panel
                    ),
                )
                .await;
            }

            self.info(
                MODULE,
                format!("[{MODULE}] Add: {name} {latitude} {longitude}"),
            )
            .await;
        }
    }

    async fn handle_cmd_update_item(&mut self, cmd_parts: &[String]) {
        if let Some(class) = cmd_parts.get(3) {
            match class.as_str() {
                "summary" => {
                    #[allow(clippy::collapsible_if)]
                    if let (Some(name), Some(time), Some(temperature), Some(weathercode)) = (
                        cmd_parts.get(4),
                        cmd_parts.get(5),
                        cmd_parts.get(6),
                        cmd_parts.get(7),
                    ) {
                        if let Some(city) = self.cities.iter_mut().find(|city| city.name == *name) {
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
                    #[allow(clippy::collapsible_if)]
                    if let (
                        Some(name),
                        Some(idx),
                        Some(time),
                        Some(temperature_2m_max),
                        Some(temperature_2m_min),
                        Some(precipitation_probability_max),
                        Some(weather_code),
                    ) = (
                        cmd_parts.get(4),
                        cmd_parts.get(5),
                        cmd_parts.get(6),
                        cmd_parts.get(7),
                        cmd_parts.get(8),
                        cmd_parts.get(9),
                        cmd_parts.get(10),
                    ) {
                        if let Some(city) = self.cities.iter_mut().find(|city| city.name == *name) {
                            let idx = idx.parse::<usize>().unwrap();
                            let daily = WeatherDaily {
                                time: time.to_string(),
                                temperature_2m_max: temperature_2m_max.parse::<f32>().unwrap(),
                                temperature_2m_min: temperature_2m_min.parse::<f32>().unwrap(),
                                precipitation_probability_max: precipitation_probability_max
                                    .parse::<u8>()
                                    .unwrap(),
                                weather_code: weather_code.parse::<u8>().unwrap(),
                            };

                            if let Some(weather) = city.weather.as_mut() {
                                if weather.daily.len() <= idx {
                                    weather.daily.resize_with(idx + 1, || WeatherDaily {
                                        time: String::new(),
                                        temperature_2m_max: 0.0,
                                        temperature_2m_min: 0.0,
                                        precipitation_probability_max: 0,
                                        weather_code: 0,
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
                    "update" => self.handle_cmd_update().await,
                    "update_item" => self.handle_cmd_update_item(&cmd_parts).await,
                    "add" => self.handle_cmd_add(&cmd_parts).await,
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
