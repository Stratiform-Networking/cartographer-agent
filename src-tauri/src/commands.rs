use crate::auth::{check_auth, logout as auth_logout, poll_for_login, request_login_url, start_login, LoginFlowStarted, LoginUrlEvent};
use crate::cloud::CloudClient;
use crate::scanner::{
    check_device_reachable, get_arp_table_ips, scan_network_with_progress, Device, ScanProgress, ScanStage,
};
use crate::scheduler::{
    clear_scan_cancel, ensure_background_scanning, get_known_devices, get_last_scan_time,
    is_scanning, merge_devices_preserving_health, persist_state, record_scan_time, request_scan_cancel,
    reset_scan_state, set_scan_interval as scheduler_set_scan_interval, stop_background_scanning,
    trigger_immediate_scan, update_known_devices, DeviceHealthResult,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub authenticated: bool,
    pub user_email: Option<String>,
    pub network_id: Option<String>,
    pub network_name: Option<String>,
    pub last_scan: Option<String>,
    pub next_scan: Option<String>,
    pub device_count: Option<usize>,
    pub scanning_in_progress: bool,
}

/// Result of a network scan, including devices and sync status
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResultResponse {
    pub devices: Vec<Device>,
    pub synced_to_cloud: bool,
}

static CLOUD_CLIENT: Mutex<Option<Arc<CloudClient>>> = Mutex::const_new(None);

async fn get_cloud_client() -> Arc<CloudClient> {
    let mut client = CLOUD_CLIENT.lock().await;
    if client.is_none() {
        *client = Some(Arc::new(CloudClient::new()));
    }
    client.as_ref().unwrap().clone()
}

#[tauri::command]
pub async fn check_auth_status() -> Result<AgentStatus, String> {
    let devices = get_known_devices().await;
    match check_auth().await {
        Ok(status) => Ok(AgentStatus {
            authenticated: status.authenticated,
            user_email: status.user_email,
            network_id: status.network_id,
            network_name: status.network_name,
            last_scan: get_last_scan_time(),
            next_scan: None,
            device_count: Some(devices.len()),
            scanning_in_progress: is_scanning(),
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// Event name for login URL notification
pub const LOGIN_URL_EVENT: &str = "login-url";

#[tauri::command]
pub async fn start_login_flow(app: AppHandle) -> Result<AgentStatus, String> {
    // Create callback to emit login URL to frontend
    let app_clone = app.clone();
    let emit_url = move |event: LoginUrlEvent| {
        if let Err(e) = app_clone.emit(LOGIN_URL_EVENT, &event) {
            tracing::warn!("Failed to emit login URL event: {}", e);
        }
    };

    match start_login(Some(emit_url)).await {
        Ok(status) => {
            // Start background scanning if authenticated
            if status.authenticated {
                tracing::info!("Login successful, starting background scanning");
                ensure_background_scanning().await;
            }
            Ok(AgentStatus {
                authenticated: status.authenticated,
                user_email: status.user_email,
                network_id: status.network_id,
                network_name: status.network_name,
                last_scan: get_last_scan_time(),
                next_scan: None,
                device_count: None,
                scanning_in_progress: is_scanning(),
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Response for the login flow that includes the verification URL
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginFlowResponse {
    pub verification_url: String,
    pub user_code: String,
    pub device_code: String,
    pub expires_in: u64,
    pub poll_interval: u64,
}

/// Start the login flow and return the verification URL immediately.
/// This allows the frontend to show the URL to the user right away.
#[tauri::command]
pub async fn request_login() -> Result<LoginFlowResponse, String> {
    match request_login_url().await {
        Ok(info) => Ok(LoginFlowResponse {
            verification_url: info.verification_url,
            user_code: info.user_code,
            device_code: info.device_code,
            expires_in: info.expires_in,
            poll_interval: info.poll_interval,
        }),
        Err(e) => Err(e.to_string()),
    }
}

/// Poll for login completion. Call this after request_login.
/// This will block until the user completes authorization or the code expires.
#[tauri::command]
pub async fn complete_login(device_code: String, expires_in: u64, poll_interval: u64) -> Result<AgentStatus, String> {
    match poll_for_login(&device_code, expires_in, poll_interval).await {
        Ok(status) => {
            // Start background scanning if authenticated
            if status.authenticated {
                tracing::info!("Login successful, starting background scanning");
                ensure_background_scanning().await;
                // Trigger immediate scan in background (handles reconnect after logout)
                trigger_immediate_scan();
            }
            Ok(AgentStatus {
                authenticated: status.authenticated,
                user_email: status.user_email,
                network_id: status.network_id,
                network_name: status.network_name,
                last_scan: get_last_scan_time(),
                next_scan: None,
                device_count: None,
                scanning_in_progress: is_scanning(),
            })
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn logout() -> Result<(), String> {
    // Stop background scanning tasks to prevent memory leaks
    stop_background_scanning().await;

    // Delete credentials
    auth_logout().await.map_err(|e| e.to_string())?;

    // Clear in-memory devices
    update_known_devices(Vec::new()).await;

    // Reset scan state (last scan time, scanning flags) so reconnecting starts fresh
    reset_scan_state();

    // Clear persisted state (devices, scan time, etc.)
    crate::persistence::clear_state().map_err(|e| e.to_string())?;

    tracing::info!("Logged out and cleared all local device data");
    Ok(())
}

/// Event name for scan progress updates
pub const SCAN_PROGRESS_EVENT: &str = "scan-progress";

/// Cancel an in-progress network scan
#[tauri::command]
pub async fn cancel_scan() -> Result<(), String> {
    request_scan_cancel();
    Ok(())
}

#[tauri::command]
pub async fn scan_network(app: AppHandle) -> Result<ScanResultResponse, String> {
    // Clear any previous cancel request
    clear_scan_cancel();

    // Emit immediate "Starting" event so frontend knows scan has begun
    let starting_progress = ScanProgress {
        stage: ScanStage::Starting,
        message: "Starting network scan...".to_string(),
        percent: Some(0),
        devices_found: None,
        elapsed_secs: 0.0,
    };
    if let Err(e) = app.emit(SCAN_PROGRESS_EVENT, &starting_progress) {
        tracing::warn!("Failed to emit starting progress event: {}", e);
    }

    // Small yield to ensure the event is processed by the frontend
    tokio::task::yield_now().await;

    // Create progress callback that emits Tauri events
    let app_clone = app.clone();
    let progress_callback: Box<dyn Fn(ScanProgress) + Send + Sync> = Box::new(move |progress| {
        if let Err(e) = app_clone.emit(SCAN_PROGRESS_EVENT, &progress) {
            tracing::warn!("Failed to emit scan progress event: {}", e);
        }
    });

    let scan_result = scan_network_with_progress(Some(progress_callback))
        .await
        .map_err(|e| format!("{}", e))?;

    tracing::info!(
        "Scan complete, found {} devices (gateway: {:?})",
        scan_result.devices.len(),
        scan_result.network_info.gateway_ip
    );

    // Record scan time
    record_scan_time();

    // Merge new devices with existing ones, preserving health data from previous health checks
    merge_devices_preserving_health(scan_result.devices.clone(), &scan_result.network_info.subnet).await;

    // Persist to disk
    persist_state().await;

    // Upload to cloud if authenticated
    let mut synced = false;
    match check_auth().await {
        Ok(status) if status.authenticated => {
            tracing::info!(
                "Authenticated as {}, uploading to network '{}'",
                status.user_email.as_deref().unwrap_or("Unknown"),
                status.network_name.as_deref().unwrap_or("Unknown")
            );
            let client = get_cloud_client().await;
            match client.upload_scan_result(&scan_result).await {
                Ok(_) => {
                    tracing::info!("Scan results synced to cloud");
                    synced = true;
                }
                Err(e) => {
                    tracing::warn!("Failed to upload scan to cloud: {}", e);
                }
            }
        }
        Ok(_) => {
            tracing::info!("Not authenticated, skipping cloud upload");
        }
        Err(e) => {
            tracing::warn!("Failed to check auth status: {}", e);
        }
    }

    Ok(ScanResultResponse {
        devices: scan_result.devices,
        synced_to_cloud: synced,
    })
}

#[tauri::command]
pub async fn get_agent_status() -> Result<AgentStatus, String> {
    let status = check_auth().await.map_err(|e| e.to_string())?;
    let devices = get_known_devices().await;

    Ok(AgentStatus {
        authenticated: status.authenticated,
        user_email: status.user_email,
        network_id: status.network_id,
        network_name: status.network_name,
        last_scan: get_last_scan_time(),
        next_scan: None,
        device_count: Some(devices.len()),
        scanning_in_progress: is_scanning(),
    })
}

/// Get the list of known devices (from last scan or persisted state)
#[tauri::command]
pub async fn get_devices() -> Result<Vec<Device>, String> {
    Ok(get_known_devices().await)
}

#[tauri::command]
pub async fn set_scan_interval(minutes: u64) -> Result<(), String> {
    scheduler_set_scan_interval(minutes);
    Ok(())
}

/// Get the application version from Cargo.toml
#[tauri::command]
pub fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
pub async fn get_network_info() -> Result<String, String> {
    crate::scanner::get_network_info().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_cloud_dashboard() -> Result<(), String> {
    let client = get_cloud_client().await;
    client.open_dashboard().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_start_at_login(enabled: bool) -> Result<(), String> {
    crate::platform::set_start_at_login(enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_start_at_login() -> Result<bool, String> {
    crate::platform::get_start_at_login().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_notifications_enabled(_enabled: bool) -> Result<(), String> {
    // Store in config file
    Ok(())
}

#[tauri::command]
pub async fn get_notifications_enabled() -> Result<bool, String> {
    // Read from config file
    Ok(true)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckStatus {
    pub total_devices: usize,
    pub healthy_devices: usize,
    pub unreachable_devices: usize,
    pub synced_to_cloud: bool,
    /// Updated device list with latest health data
    pub devices: Vec<Device>,
}

#[tauri::command]
pub async fn run_health_check() -> Result<HealthCheckStatus, String> {
    let mut devices = get_known_devices().await;

    if devices.is_empty() {
        return Err("No devices to check. Run a scan first.".to_string());
    }

    tracing::info!("Running manual health check on {} devices", devices.len());

    // Get current ARP table for fallback checking
    // This helps detect devices that block ICMP but are still on the network
    let arp_ips = get_arp_table_ips().await;
    tracing::debug!("ARP table has {} entries for fallback", arp_ips.len());

    // Check all known devices using ping with ARP fallback
    // Update the devices in-place to ensure all devices get their response_time_ms updated
    let mut health_results = Vec::new();
    for device in &mut devices {
        let result = check_device_reachable(&device.ip, &arp_ips).await;
        let reachable = result.is_ok();
        let response_time = if reachable {
            result.ok()
        } else {
            None // Mark as unreachable
        };
        
        // Update the device's response time directly
        device.response_time_ms = response_time;
        
        health_results.push(DeviceHealthResult {
            ip: device.ip.clone(),
            reachable,
            response_time_ms: response_time,
        });
    }

    let healthy_count = health_results.iter().filter(|r| r.reachable).count();
    let unreachable_count = health_results.len() - healthy_count;

    tracing::info!(
        "Health check complete: {} healthy, {} unreachable",
        healthy_count,
        unreachable_count
    );

    // Update in-memory known devices with the updated health data
    update_known_devices(devices.clone()).await;

    // Persist to disk
    persist_state().await;

    // Upload to cloud if authenticated
    let mut synced = false;
    match check_auth().await {
        Ok(status) if status.authenticated => {
            let client = get_cloud_client().await;
            match client.upload_health_check(&health_results).await {
                Ok(_) => {
                    tracing::info!("Health check results synced to cloud");
                    synced = true;
                }
                Err(e) => {
                    tracing::warn!("Failed to upload health check to cloud: {}", e);
                }
            }
        }
        Ok(_) => {
            tracing::info!("Not authenticated, skipping cloud upload");
        }
        Err(e) => {
            tracing::warn!("Failed to check auth status: {}", e);
        }
    }

    Ok(HealthCheckStatus {
        total_devices: health_results.len(),
        healthy_devices: healthy_count,
        unreachable_devices: unreachable_count,
        synced_to_cloud: synced,
        devices,
    })
}

