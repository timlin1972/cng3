use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDateTime};
use sysinfo::System;

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

pub fn ts_str_full(ts: u64) -> String {
    let datetime_local: DateTime<Local> = DateTime::from_timestamp(ts as i64, 0)
        .unwrap_or_else(|| panic!("Failed to parse ts ({ts})"))
        .with_timezone(&Local);

    datetime_local.format("%Y-%m-%d %H:%M:%S %:z").to_string()
}

pub fn uptime() -> u64 {
    System::uptime()
}

pub fn uptime_str(uptime: u64) -> String {
    let mut uptime = uptime;
    let days = uptime / 86400;
    uptime -= days * 86400;
    let hours = uptime / 3600;
    uptime -= hours * 3600;
    let minutes = uptime / 60;
    let seconds = uptime % 60;

    format!("{days}d {hours:02}:{minutes:02}:{seconds:02}")
}

pub fn format_number(num: u64) -> String {
    if num >= 1_000_000 {
        format!("{:.1}M", num as f64 / 1_000_000.0)
    } else if num >= 1_000 {
        format!("{:.1}k", num as f64 / 1_000.0)
    } else {
        num.to_string()
    }
}

fn format_speed(num: f64) -> String {
    if num >= 1_000_000_000.0 {
        format!("{:.1}GB/s", num / 1_000_000_000.0)
    } else if num >= 1_000_000.0 {
        format!("{:.1}MB/s", num / 1_000_000.0)
    } else if num >= 1_000.0 {
        format!("{:.1}KB/s", num / 1_000.0)
    } else {
        format!("{:.1}B/s", num)
    }
}

pub fn transmit_str(transmit_size: u64, escaped_time: u64) -> String {
    let escaped_time = if escaped_time == 0 { 1 } else { escaped_time };
    let speed = transmit_size as f64 / escaped_time as f64;

    format!(
        "{} ({}, {escaped_time}s)",
        format_speed(speed),
        format_number(transmit_size)
    )
}

pub fn datetime_str_to_ts(datetime_str: &str) -> i64 {
    let naive_datetime = NaiveDateTime::parse_from_str(datetime_str, "%Y-%m-%dT%H:%M")
        .expect("解析日期時間字串失敗");
    naive_datetime.and_utc().timestamp()
}
