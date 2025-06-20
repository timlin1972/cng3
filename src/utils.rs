use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};

pub fn ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch!")
        .as_secs()
}

pub fn ts_str(ts: u64) -> String {
    let datetime_local: DateTime<Local> = DateTime::from_timestamp(ts as i64, 0)
        .unwrap_or_else(|| panic!("Failed to parse ts ({ts})"))
        .with_timezone(&Local);

    datetime_local.format("%H:%M:%S").to_string()
}
