use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio::sync::mpsc::Sender;

use crate::messages::{ACTION_INIT, ACTION_SHOW, Cmd, Data, Msg};
use crate::plugins::plugins_main::{self, Plugin};
use crate::utils::{
    self,
    weather::{self, City},
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
        utils::log::log_new(&msg_tx, MODULE).await;

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

        let mut cities = std::mem::take(&mut self.cities);
        for city in &mut cities {
            let weather = weather::get_weather(city.latitude, city.longitude).await;
            if let Ok(weather) = weather {
                self.cmd(
                    MODULE,
                    format!(
                        "p {} weather update summary {} {} {} {}",
                        self.gui_panel,
                        city.name,
                        weather.time,
                        weather.temperature,
                        weather.weathercode
                    ),
                )
                .await;

                for (idx, daily) in weather.daily.iter().enumerate() {
                    self.cmd(
                        MODULE,
                        format!(
                            "p {} weather update daily {} {} {} {} {} {} {}",
                            self.gui_panel,
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
                }

                city.weather = Some(weather);
            }
        }
        self.cities = cities;

        self.info(MODULE, format!("[{MODULE}] updated")).await;
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
        self.info(MODULE, format!("{:<12} {:<7}", "Name", "Temper"))
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
