#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use tauri::{LogicalSize, Manager, PhysicalPosition, Size};
use thiserror::Error;

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
    #[derive(serde::Deserialize)]
    struct TimeApiResponse {
        #[serde(rename = "unixTime")]
        unix_time: i64,
    }

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

    let payload: TimeApiResponse = response.json().await.map_err(|_| TimeSyncError::Parse)?;

    Ok(TimeSyncResult {
        epoch_millis: payload.unix_time * 1000,
    })
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window unavailable");
            window.set_always_on_top(true)?;
            let desired_size = LogicalSize::new(340.0, 340.0);
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
