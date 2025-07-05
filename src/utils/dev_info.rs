use crate::utils;

// DevInfo
#[derive(Debug, Clone)]
pub struct DevInfo {
    pub ts: u64,
    pub name: String,
    pub onboard: bool,
    pub version: Option<String>,
    pub tailscale_ip: Option<String>,
    pub temperature: Option<f32>,
    pub app_uptime: Option<u64>,
}

pub fn onboard_str(onboard: bool) -> &'static str {
    if onboard { "on" } else { "off" }
}

pub fn temperature_str(temperature: Option<f32>) -> String {
    if let Some(t) = temperature {
        format!("{:.1}°C", t)
    } else {
        "n/a".to_owned()
    }
}

pub fn app_uptime_str(app_uptime: Option<u64>) -> String {
    if let Some(t) = app_uptime {
        utils::time::uptime_str(t)
    } else {
        "n/a".to_owned()
    }
}
