use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WeatherDaily {
    pub time: String,
    pub temperature_2m_max: f32,
    pub temperature_2m_min: f32,
    pub precipitation_probability_max: u8,
    pub weather_code: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Weather {
    pub time: String,
    pub temperature: f32,
    pub weathercode: u8,
    pub daily: Vec<WeatherDaily>,
}

#[derive(Debug, Clone)]
pub struct City {
    pub name: String,
    pub latitude: f32,
    pub longitude: f32,
    pub weather: Option<Weather>,
}

pub async fn get_weather(latitude: f32, longitude: f32) -> Result<Weather, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={latitude}&longitude={longitude}&daily=temperature_2m_max,temperature_2m_min,precipitation_probability_max,weather_code&current_weather=true"
    );

    let response = client
        .get(url)
        .timeout(tokio::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to get weather: {e}"))?;

    let body = response
        .text()
        .await
        .map_err(|e| format!("Failed to get weather: {e}"))?;

    let weather_data: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse weather data: {e}"))?;

    let current = &weather_data["current_weather"];
    let time = current["time"]
        .as_str()
        .ok_or("Missing current_weather.time")?
        .to_string();
    let temperature = current["temperature"]
        .as_f64()
        .ok_or("Missing or invalid current_weather.temperature")? as f32;
    let weathercode = current["weathercode"]
        .as_u64()
        .ok_or("Missing or invalid current_weather.weathercode")? as u8;

    let (max_temps, min_temps, precip_probs, weather_codes, dates) = (
        weather_data["daily"]["temperature_2m_max"]
            .as_array()
            .ok_or("Missing daily.temperature_2m_max")?,
        weather_data["daily"]["temperature_2m_min"]
            .as_array()
            .ok_or("Missing daily.temperature_2m_min")?,
        weather_data["daily"]["precipitation_probability_max"]
            .as_array()
            .ok_or("Missing daily.precipitation_probability_max")?,
        weather_data["daily"]["weather_code"]
            .as_array()
            .ok_or("Missing daily.weather_code")?,
        weather_data["daily"]["time"]
            .as_array()
            .ok_or("Missing daily.time")?,
    );

    let len = max_temps.len();
    if min_temps.len() != len
        || precip_probs.len() != len
        || weather_codes.len() != len
        || dates.len() != len
    {
        return Err("Mismatch in forecast array lengths".to_string());
    }

    let mut daily_forecast = Vec::new();
    for i in 0..len {
        let daily = WeatherDaily {
            time: dates[i].as_str().ok_or("Invalid daily.time")?.to_string(),
            temperature_2m_max: max_temps[i].as_f64().ok_or("Invalid temperature_2m_max")? as f32,
            temperature_2m_min: min_temps[i].as_f64().ok_or("Invalid temperature_2m_min")? as f32,
            precipitation_probability_max: precip_probs[i]
                .as_f64()
                .ok_or("Invalid precipitation_probability_max")?
                as u8,
            weather_code: weather_codes[i].as_u64().ok_or("Invalid weather_code")? as u8,
        };
        daily_forecast.push(daily);
    }

    Ok(Weather {
        time,
        temperature,
        weathercode,
        daily: daily_forecast,
    })
}

const WEATHER_CODES: [(u8, &str); 28] = [
    (0, "晴天"),
    (1, "多雲時晴"),
    (2, "局部多雲"),
    (3, "陰天"),
    (45, "有霧"),
    (48, "凍霧"),
    (51, "毛毛雨（小）"),
    (53, "毛毛雨（中）"),
    (55, "毛毛雨（大）"),
    (56, "凍雨（小）"),
    (57, "凍雨（大）"),
    (61, "小雨"),
    (63, "中雨"),
    (65, "大雨"),
    (66, "凍雨（小雨）"),
    (67, "凍雨（大雨）"),
    (71, "小雪"),
    (73, "中雪"),
    (75, "大雪"),
    (77, "雪粒"),
    (80, "小陣雨"),
    (81, "中陣雨"),
    (82, "強陣雨"),
    (85, "小陣雪"),
    (86, "大陣雪"),
    (95, "雷雨"),
    (96, "雷雨夾小冰雹"),
    (99, "雷雨夾大冰雹"),
];

pub fn weather_code_str(code: u8) -> &'static str {
    WEATHER_CODES
        .iter()
        .find(|&&(c, _)| c == code)
        .map(|&(_, desc)| desc)
        .unwrap_or("未知天氣")
}

const WEATHER_CODES_EMOJI: [(u8, &str); 28] = [
    (0, "☀️"),
    (1, "🌤️"),
    (2, "⛅"),
    (3, "☁️"),
    (45, "🌫️"),
    (48, "❄️"),
    (51, "🌧️"),
    (53, "🌧️"),
    (55, "🌧️"),
    (56, "❄️"),
    (57, "❄️"),
    (61, "🌧️"),
    (63, "🌧️"),
    (65, "🌧️"),
    (66, "❄️"),
    (67, "❄️"),
    (71, "🌨️"),
    (73, "🌨️"),
    (75, "🌨️"),
    (77, "❄️"),
    (80, "🌦️"),
    (81, "🌦️"),
    (82, "🌦️"),
    (85, "🌨️"),
    (86, "🌨️"),
    (95, "⛈️"),
    (96, "⛈️"),
    (99, "⛈️"),
];

pub fn weather_code_emoji(code: u8) -> &'static str {
    WEATHER_CODES_EMOJI
        .iter()
        .find(|&&(c, _)| c == code)
        .map(|&(_, desc)| desc)
        .unwrap_or("未知天氣")
}
