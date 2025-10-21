#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Serialize;
use serde_json::Value;
use tauri::{LogicalSize, Manager, PhysicalPosition, Size};
use thiserror::Error;

const MILLIS_PER_SECOND: i64 = 1000;

#[derive(Debug, Error)]
enum TimeSyncError {
    #[error("network request failed: {0}")]
    Request(String),
    #[error("failed to parse response")]
    Parse,
}

#[derive(Serialize)]
struct TimeSyncResult {
    epoch_millis: i64,
}

#[tauri::command]
async fn sync_time(time_zone: Option<String>) -> Result<TimeSyncResult, String> {
    let zone = time_zone.unwrap_or_else(|| "Etc/UTC".to_string());
    fetch_remote_time(&zone)
        .await
        .or_else(|err| {
            eprintln!("time sync fallback triggered: {err}");
            Ok(TimeSyncResult {
                epoch_millis: Utc::now().timestamp_millis(),
            })
        })
        .map_err(|err: TimeSyncError| err.to_string())
}

async fn fetch_remote_time(zone: &str) -> Result<TimeSyncResult, TimeSyncError> {
    let encoded_zone = urlencoding::encode(zone);
    let url = format!(
        "https://timeapi.io/api/Time/current/zone?timeZone={}",
        encoded_zone
    );

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|err| TimeSyncError::Request(err.to_string()))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|err| TimeSyncError::Request(err.to_string()))?;

    if !response.status().is_success() {
        return Err(TimeSyncError::Request(format!(
            "unexpected status: {}",
            response.status()
        )));
    }

    let payload: Value = response.json().await.map_err(|_| TimeSyncError::Parse)?;
    let epoch_millis = extract_epoch_millis(&payload).ok_or(TimeSyncError::Parse)?;

    Ok(TimeSyncResult { epoch_millis })
}

fn extract_epoch_millis(payload: &Value) -> Option<i64> {
    if let Some(unix_seconds) = payload.get("unixTime").and_then(value_to_i64) {
        return Some(unix_seconds * MILLIS_PER_SECOND);
    }

    for key in [
        "dateTime",
        "dateTimeUtc",
        "currentLocalTime",
        "currentUtcTime",
    ] {
        if let Some(candidate) = payload.get(key).and_then(Value::as_str) {
            if let Some(parsed) = parse_iso_candidate(candidate) {
                return Some(parsed);
            }
        }
    }

    let year = payload.get("year").and_then(value_to_i64)?;
    let month = payload.get("month").and_then(value_to_i64)?;
    let day = payload.get("day").and_then(value_to_i64)?;
    let hour = payload.get("hour").and_then(value_to_i64)?;
    let minute = payload.get("minute").and_then(value_to_i64)?;
    let seconds = payload.get("seconds").and_then(value_to_i64)?;
    let millis = payload
        .get("milliSeconds")
        .and_then(value_to_i64)
        .unwrap_or(0);

    let year_i32 = i32::try_from(year).ok()?;
    let month_u32 = u32::try_from(month).ok()?;
    let day_u32 = u32::try_from(day).ok()?;
    let hour_u32 = u32::try_from(hour).ok()?;
    let minute_u32 = u32::try_from(minute).ok()?;
    let second_u32 = u32::try_from(seconds).ok()?;
    let millis_u32 = u32::try_from(millis).ok()?;

    let date = NaiveDate::from_ymd_opt(year_i32, month_u32, day_u32)?;
    let time = NaiveTime::from_hms_milli_opt(hour_u32, minute_u32, second_u32, millis_u32)?;

    Some(NaiveDateTime::new(date, time).and_utc().timestamp_millis())
}

fn parse_iso_candidate(value: &str) -> Option<i64> {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(value) {
        return Some(parsed.timestamp_millis());
    }

    for format in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(value, format) {
            return Some(naive.and_utc().timestamp_millis());
        }
    }

    None
}

fn value_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float.round() as i64)),
        Value::String(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .map(|float| float.round() as i64),
        _ => None,
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window unavailable");
            window.set_always_on_top(true)?;
            window.set_visible_on_all_workspaces(true)?;
            let desired_size = LogicalSize::new(600.0, 600.0);
            window.set_size(Size::Logical(desired_size))?;
            let margin = 24.0;

            if let Some(monitor) = window.current_monitor()? {
                let monitor_size = monitor.size();
                let outer_size = window.outer_size()?;
                let x = (monitor_size.width as f64 - outer_size.width as f64 - margin).max(0.0);
                let y = (monitor_size.height as f64 - outer_size.height as f64 - margin).max(0.0);
                window.set_position(tauri::Position::Physical(PhysicalPosition {
                    x: x.round() as i32,
                    y: y.round() as i32,
                }))?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![sync_time])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
