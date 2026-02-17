use crate::auth::check_auth;
use crate::cloud::{CloudClient, ResultReport};
use crate::commands::SCAN_PROGRESS_EVENT;
use crate::persistence;
use crate::scanner::{check_device_reachable, get_arp_table_ips, scan_network_with_progress, Device, ScanProgress};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};
use tokio_util::sync::CancellationToken;

/// Event name for health check progress updates
pub const HEALTH_CHECK_PROGRESS_EVENT: &str = "health-check-progress";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheckProgress {
    pub stage: HealthCheckStage,
    pub message: String,
    pub total_devices: usize,
    pub checked_devices: usize,
    pub healthy_devices: usize,
    /// Whether the health check was successfully synced to cloud (only set on Complete stage)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synced_to_cloud: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthCheckStage {
    Starting,
    CheckingDevices,
    Uploading,
    Complete,
}

static SCAN_INTERVAL: AtomicU64 = AtomicU64::new(300); // Default 5 minutes
static HEALTH_CHECK_INTERVAL: AtomicU64 = AtomicU64::new(60); // Default 1 minute

// Track if background tasks are already running
static BACKGROUND_RUNNING: AtomicBool = AtomicBool::new(false);

// Track if a network scan is currently in progress
static SCANNING_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// Track if a health check is currently in progress
static HEALTH_CHECK_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// Flag to request scan cancellation
static SCAN_CANCEL_REQUESTED: AtomicBool = AtomicBool::new(false);

// Cached list of known devices for health checks
static KNOWN_DEVICES: Mutex<Vec<Device>> = Mutex::const_new(Vec::new());

// Last scan timestamp (Unix timestamp in seconds)
static LAST_SCAN_TIME: AtomicU64 = AtomicU64::new(0);

// Global AppHandle for starting background tasks from anywhere
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

// Cancellation token for stopping background tasks on logout
static BACKGROUND_CANCEL_TOKEN: OnceLock<Mutex<Option<CancellationToken>>> = OnceLock::new();

// Shared CloudClient for all background tasks (avoids creating new HTTP clients)
static SHARED_CLOUD_CLIENT: OnceLock<Arc<CloudClient>> = OnceLock::new();

fn get_cancel_token_store() -> &'static Mutex<Option<CancellationToken>> {
    BACKGROUND_CANCEL_TOKEN.get_or_init(|| Mutex::const_new(None))
}

fn get_shared_cloud_client() -> Arc<CloudClient> {
    SHARED_CLOUD_CLIENT.get_or_init(|| Arc::new(CloudClient::new())).clone()
}

pub fn init(app: AppHandle) {
    APP_HANDLE.set(app).ok();

    // Load persisted state
    if let Ok(state) = persistence::load_state() {
        LAST_SCAN_TIME.store(state.last_scan_time, Ordering::Relaxed);
        if state.scan_interval_minutes > 0 {
            SCAN_INTERVAL.store(state.scan_interval_minutes * 60, Ordering::Relaxed);
        }
        if state.health_check_interval_seconds > 0 {
            HEALTH_CHECK_INTERVAL.store(state.health_check_interval_seconds, Ordering::Relaxed);
        }
        // Load devices into memory (spawn async task)
        if !state.devices.is_empty() {
            let devices = state.devices;
            tauri::async_runtime::spawn(async move {
                let mut known = KNOWN_DEVICES.lock().await;
                *known = devices;
            });
        }
    }

    tracing::info!("Scheduler initialized");
}

/// Get the stored AppHandle
pub fn get_app_handle() -> Option<AppHandle> {
    APP_HANDLE.get().cloned()
}

/// Start background scanning if not already running (can be called from anywhere)
pub async fn ensure_background_scanning() {
    if let Some(app) = get_app_handle() {
        start_background_scanning(app).await;
    }
}

/// Trigger an immediate full scan and health check in the background.
/// This returns immediately and does not block the caller.
/// Use this when reconnecting to cloud after logout.
pub fn trigger_immediate_scan() {
    if is_scanning() {
        tracing::debug!("Scan already in progress, skipping immediate scan");
        return;
    }

    if let Some(app) = get_app_handle() {
        tracing::info!("Triggering immediate scan (reconnect to cloud)");
        // Spawn in background so we don't block the login flow
        tokio::spawn(async move {
            run_initial_scan_sequence(&app).await;
        });
    }
}

pub fn set_scan_interval(minutes: u64) {
    SCAN_INTERVAL.store(minutes * 60, Ordering::Relaxed);
    tracing::info!("Scan interval set to {} minutes", minutes);
}

pub fn get_scan_interval() -> u64 {
    SCAN_INTERVAL.load(Ordering::Relaxed) / 60
}

pub fn set_health_check_interval(seconds: u64) {
    HEALTH_CHECK_INTERVAL.store(seconds, Ordering::Relaxed);
    tracing::info!("Health check interval set to {} seconds", seconds);
}

pub fn get_health_check_interval() -> u64 {
    HEALTH_CHECK_INTERVAL.load(Ordering::Relaxed)
}

/// Update the list of known devices (called after successful scans)
pub async fn update_known_devices(devices: Vec<Device>) {
    let mut known = KNOWN_DEVICES.lock().await;
    *known = devices;
}

/// Normalize a MAC address to lowercase colon-separated format (e.g. "aa:bb:cc:dd:ee:ff").
fn normalize_mac(mac: &str) -> String {
    let hex: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_lowercase();
    hex.as_bytes()
        .chunks(2)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
        .collect::<Vec<&str>>()
        .join(":")
}

/// Merge new devices with existing ones, preserving health data from previous health checks.
/// Uses dual-key matching (IP first, then MAC) to correctly identify devices even when
/// IP or MAC changes (DHCP churn, NIC replacement, etc.).
/// - If the new device has no response_time_ms, preserve the old one
/// - If the new device has response_time_ms, use the new value
/// - If IP match found but MAC differs, update the MAC
/// - If no IP match but MAC matches, treat as same device with IP change
/// Devices not matched by either key are kept but marked as offline (response_time_ms = None),
/// but only if they are within the target subnet. Out-of-subnet devices are dropped to avoid
/// retaining stale entries from other interfaces (VPN, containers, virtual adapters).
pub async fn merge_devices_preserving_health(new_devices: Vec<Device>, subnet: &str) {
    let mut known = KNOWN_DEVICES.lock().await;

    // Parse subnet for filtering stale offline devices from other interfaces
    let subnet_network: Option<ipnetwork::IpNetwork> = match subnet.parse() {
        Ok(net) => Some(net),
        Err(e) => {
            tracing::warn!("Could not parse subnet '{}' for merge filtering: {}", subnet, e);
            None
        }
    };

    // Create maps for dual-key lookup
    let old_device_map: std::collections::HashMap<String, Device> =
        known.iter().map(|d| (d.ip.clone(), d.clone())).collect();

    let old_device_map_by_mac: std::collections::HashMap<String, Device> = known
        .iter()
        .filter_map(|d| {
            d.mac
                .as_ref()
                .map(|m| (normalize_mac(m), d.clone()))
        })
        .collect();

    // Track which old device IPs were matched (by either IP or MAC)
    let mut matched_old_ips: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    // Merge: for each new device, try IP match first, then MAC match
    let mut merged: Vec<Device> = new_devices
        .into_iter()
        .map(|mut new_device| {
            // Try IP match first
            if let Some(old_device) = old_device_map.get(&new_device.ip) {
                matched_old_ips.insert(old_device.ip.clone());

                // Preserve health data if new device doesn't have it
                if new_device.response_time_ms.is_none()
                    || new_device.response_time_ms == Some(0.0)
                {
                    if old_device.response_time_ms.is_some() {
                        new_device.response_time_ms = old_device.response_time_ms;
                    }
                }
                // Preserve hostname if new doesn't have one
                if new_device.hostname.is_none() && old_device.hostname.is_some() {
                    new_device.hostname = old_device.hostname.clone();
                }

                // Update MAC if it changed (e.g. NIC replacement)
                if let Some(ref new_mac) = new_device.mac {
                    let new_mac_norm = normalize_mac(new_mac);
                    let old_mac_norm = old_device.mac.as_ref().map(|m| normalize_mac(m));
                    if old_mac_norm.as_deref() != Some(&new_mac_norm) {
                        tracing::info!(
                            "Device {} MAC changed from {:?} to {}",
                            new_device.ip,
                            old_device.mac,
                            new_mac
                        );
                    }
                }
            } else if let Some(ref new_mac) = new_device.mac {
                // No IP match — try MAC match (DHCP churn: same device, new IP)
                let new_mac_norm = normalize_mac(new_mac);
                if let Some(old_device) = old_device_map_by_mac.get(&new_mac_norm) {
                    matched_old_ips.insert(old_device.ip.clone());
                    tracing::info!(
                        "Device MAC {} moved from IP {} to {}",
                        new_mac,
                        old_device.ip,
                        new_device.ip
                    );

                    // Preserve health data
                    if new_device.response_time_ms.is_none()
                        || new_device.response_time_ms == Some(0.0)
                    {
                        if old_device.response_time_ms.is_some() {
                            new_device.response_time_ms = old_device.response_time_ms;
                        }
                    }
                    // Preserve hostname if new doesn't have one
                    if new_device.hostname.is_none() && old_device.hostname.is_some() {
                        new_device.hostname = old_device.hostname.clone();
                    }
                    // Preserve vendor if new doesn't have one
                    if new_device.vendor.is_none() && old_device.vendor.is_some() {
                        new_device.vendor = old_device.vendor.clone();
                    }
                    // Preserve device_type if new doesn't have one
                    if new_device.device_type.is_none() && old_device.device_type.is_some() {
                        new_device.device_type = old_device.device_type.clone();
                    }
                }
            }
            new_device
        })
        .collect();

    // Add old devices that weren't matched by either IP or MAC, marking them as offline.
    // Only retain offline devices that are within the target subnet to avoid keeping
    // stale entries from other interfaces (VPN, containers, virtual adapters).
    for old_device in old_device_map.values() {
        if !matched_old_ips.contains(&old_device.ip) {
            // Drop out-of-subnet devices instead of keeping them as offline
            if let Some(ref network) = subnet_network {
                if let Ok(ip) = old_device.ip.parse::<std::net::IpAddr>() {
                    if !network.contains(ip) {
                        tracing::info!(
                            "Dropping out-of-subnet device {} (not in {})",
                            old_device.ip,
                            subnet
                        );
                        continue;
                    }
                }
            }

            tracing::info!(
                "Device {} not found in scan, marking as offline",
                old_device.ip
            );
            let offline_device = Device {
                ip: old_device.ip.clone(),
                mac: old_device.mac.clone(),
                response_time_ms: None, // Mark as offline
                hostname: old_device.hostname.clone(),
                vendor: old_device.vendor.clone(),
                device_type: old_device.device_type.clone(),
            };
            merged.push(offline_device);
        }
    }

    *known = merged;
}

/// Get current known devices
pub async fn get_known_devices() -> Vec<Device> {
    KNOWN_DEVICES.lock().await.clone()
}

/// Record that a scan just completed and persist to disk
pub fn record_scan_time() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    LAST_SCAN_TIME.store(now, Ordering::Relaxed);
}

/// Persist current state to disk (call after scans)
pub async fn persist_state() {
    let devices = get_known_devices().await;
    let scan_time = LAST_SCAN_TIME.load(Ordering::Relaxed);

    if let Err(e) = persistence::save_scan_results(&devices, scan_time) {
        tracing::warn!("Failed to persist state: {}", e);
    }
}

/// Get the last scan time as ISO string, or None if never scanned
pub fn get_last_scan_time() -> Option<String> {
    let timestamp = LAST_SCAN_TIME.load(Ordering::Relaxed);
    if timestamp == 0 {
        return None;
    }
    let datetime = chrono::DateTime::from_timestamp(timestamp as i64, 0)?;
    Some(datetime.to_rfc3339())
}

/// Check if background tasks are running
pub fn is_background_running() -> bool {
    BACKGROUND_RUNNING.load(Ordering::Relaxed)
}

/// Check if a network scan is currently in progress
pub fn is_scanning() -> bool {
    SCANNING_IN_PROGRESS.load(Ordering::Relaxed)
}

/// Check if a health check is currently in progress
pub fn is_health_checking() -> bool {
    HEALTH_CHECK_IN_PROGRESS.load(Ordering::Relaxed)
}

/// Check if any scan or health check operation is in progress
pub fn is_busy() -> bool {
    is_scanning() || is_health_checking()
}

/// Stop all background scanning tasks.
/// Call this on logout to prevent memory leaks from orphaned tasks.
pub async fn stop_background_scanning() {
    let mut token_guard = get_cancel_token_store().lock().await;
    if let Some(token) = token_guard.take() {
        tracing::info!("Stopping background scanning tasks");
        token.cancel();
        // Reset the running flag so tasks can be restarted on next login
        BACKGROUND_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Reset all scan-related state to defaults.
/// Called during logout to ensure a clean slate when signing back in.
pub fn reset_scan_state() {
    LAST_SCAN_TIME.store(0, Ordering::Relaxed);
    SCANNING_IN_PROGRESS.store(false, Ordering::SeqCst);
    HEALTH_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
    SCAN_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
    tracing::info!("Reset all scan state (last scan time, scanning flags)");
}

/// Request cancellation of the current scan
pub fn request_scan_cancel() {
    SCAN_CANCEL_REQUESTED.store(true, Ordering::SeqCst);
    tracing::info!("Scan cancellation requested");
}

/// Check if scan cancellation has been requested
pub fn is_scan_cancelled() -> bool {
    SCAN_CANCEL_REQUESTED.load(Ordering::Relaxed)
}

/// Clear the cancellation flag (call when starting a new scan)
pub fn clear_scan_cancel() {
    SCAN_CANCEL_REQUESTED.store(false, Ordering::SeqCst);
}

/// Helper to run a single scan and upload
async fn run_scan_and_upload(app: &AppHandle) {
    tracing::info!("Running network scan");

    // Mark scan as in progress
    SCANNING_IN_PROGRESS.store(true, Ordering::SeqCst);

    // Create progress callback that emits Tauri events
    let app_clone = app.clone();
    let progress_callback: Box<dyn Fn(ScanProgress) + Send + Sync> = Box::new(move |progress| {
        if let Err(e) = app_clone.emit(SCAN_PROGRESS_EVENT, &progress) {
            tracing::warn!("Failed to emit scan progress event: {}", e);
        }
    });

    match scan_network_with_progress(Some(progress_callback)).await {
        Ok(scan_result) => {
            let device_count = scan_result.devices.len();
            tracing::info!(
                "Scan found {} devices (gateway: {:?})",
                device_count,
                scan_result.network_info.gateway_ip
            );

            // Record scan time
            record_scan_time();

            // Merge new devices with existing ones, preserving health data
            merge_devices_preserving_health(scan_result.devices.clone(), &scan_result.network_info.subnet).await;

            // Persist to disk
            persist_state().await;

            // Upload to cloud if authenticated
            match check_auth().await {
                Ok(status) if status.authenticated => {
                    tracing::debug!(
                        "Authenticated as {}, uploading scan",
                        status.user_email.as_deref().unwrap_or("unknown")
                    );
                    let client = get_shared_cloud_client();
                    if let Err(e) = client.upload_scan_result(&scan_result).await {
                        tracing::warn!("Failed to upload scan to cloud: {}", e);
                    } else {
                        tracing::info!("Scan synced to cloud");
                    }
                }
                Ok(_) => {
                    tracing::debug!("Not authenticated, skipping cloud sync");
                }
                Err(e) => {
                    tracing::warn!("Auth check failed during scan upload: {}", e);
                }
            }

            // Emit event for UI update
            if let Err(e) = app.emit("scan-complete", device_count) {
                tracing::warn!("Failed to emit scan-complete event: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("Scan failed: {}", e);
        }
    }

    // Mark scan as complete
    SCANNING_IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// Helper to run health checks and upload
async fn run_health_checks_and_upload(app: &AppHandle) {
    // Skip if a scan or health check is already in progress
    if is_scanning() {
        tracing::debug!("Skipping health check - network scan in progress");
        return;
    }
    if HEALTH_CHECK_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        tracing::debug!("Skipping health check - already in progress");
        return;
    }

    let mut devices = get_known_devices().await;
    if devices.is_empty() {
        HEALTH_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
        return;
    }

    tracing::debug!("Running health checks on {} devices", devices.len());

    // Get fresh ARP table for fallback checking
    // Devices that block ICMP (common on Windows) but are in ARP cache are still reachable
    let arp_ips = get_arp_table_ips().await;

    let mut health_results = Vec::new();
    for device in &mut devices {
        let result = check_device_reachable(&device.ip, &arp_ips).await;
        let reachable = result.is_ok();
        let response_time = if reachable { result.ok() } else { None };
        
        // Update the device's response time
        device.response_time_ms = response_time;
        
        health_results.push(DeviceHealthResult {
            ip: device.ip.clone(),
            reachable,
            response_time_ms: response_time,
        });
    }

    // Update in-memory devices with updated health data
    update_known_devices(devices).await;

    // Persist to disk
    persist_state().await;

    // Upload health results to cloud if authenticated
    match check_auth().await {
        Ok(status) if status.authenticated => {
            let client = get_shared_cloud_client();
            if let Err(e) = client.upload_health_check(&health_results).await {
                tracing::debug!("Failed to upload health check: {}", e);
            }
        }
        Ok(_) => {
            tracing::debug!("Not authenticated, skipping health check cloud sync");
        }
        Err(e) => {
            tracing::debug!("Auth check failed during health upload: {}", e);
        }
    }

    // Emit event for UI update
    let healthy_count = health_results.iter().filter(|r| r.reachable).count();
    if let Err(e) = app.emit("health-check-complete", (healthy_count, health_results.len())) {
        tracing::debug!("Failed to emit health-check-complete event: {}", e);
    }

    // Mark health check as complete
    HEALTH_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// Helper to run health checks with progress events
async fn run_health_checks_with_progress(app: &AppHandle) {
    // Skip if a scan is in progress (but allow during initial sequence)
    if is_scanning() {
        tracing::debug!("Skipping health check with progress - network scan in progress");
        return;
    }
    
    // Skip if another health check is already running
    if HEALTH_CHECK_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        tracing::debug!("Skipping health check with progress - already in progress");
        return;
    }

    let mut devices = get_known_devices().await;
    let total = devices.len();

    if total == 0 {
        // Emit complete event even with no devices
        let _ = app.emit(
            HEALTH_CHECK_PROGRESS_EVENT,
            HealthCheckProgress {
                stage: HealthCheckStage::Complete,
                message: "No devices to check".to_string(),
                total_devices: 0,
                checked_devices: 0,
                healthy_devices: 0,
                synced_to_cloud: Some(false),
            },
        );
        HEALTH_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
        return;
    }

    tracing::info!("Running health checks with progress on {} devices", total);

    // Emit starting event
    let _ = app.emit(
        HEALTH_CHECK_PROGRESS_EVENT,
        HealthCheckProgress {
            stage: HealthCheckStage::Starting,
            message: format!("Checking {} devices...", total),
            total_devices: total,
            checked_devices: 0,
            healthy_devices: 0,
            synced_to_cloud: None,
        },
    );

    // Get fresh ARP table for fallback checking
    // Devices that block ICMP (common on Windows) but are in ARP cache are still reachable
    let arp_ips = get_arp_table_ips().await;

    let mut health_results = Vec::new();
    let mut healthy_count = 0;

    for (i, device) in devices.iter_mut().enumerate() {
        let result = check_device_reachable(&device.ip, &arp_ips).await;
        let reachable = result.is_ok();
        let response_time = if reachable { result.ok() } else { None };

        if reachable {
            healthy_count += 1;
        }

        // Update the device's response time
        device.response_time_ms = response_time;

        health_results.push(DeviceHealthResult {
            ip: device.ip.clone(),
            reachable,
            response_time_ms: response_time,
        });

        // Emit progress every device
        let _ = app.emit(
            HEALTH_CHECK_PROGRESS_EVENT,
            HealthCheckProgress {
                stage: HealthCheckStage::CheckingDevices,
                message: format!("Checking {}...", device.ip),
                total_devices: total,
                checked_devices: i + 1,
                healthy_devices: healthy_count,
                synced_to_cloud: None,
            },
        );
    }

    // Update in-memory devices with updated health data
    update_known_devices(devices).await;

    // Persist to disk
    persist_state().await;

    // Emit uploading event
    let _ = app.emit(
        HEALTH_CHECK_PROGRESS_EVENT,
        HealthCheckProgress {
            stage: HealthCheckStage::Uploading,
            message: "Syncing results to cloud...".to_string(),
            total_devices: total,
            checked_devices: total,
            healthy_devices: healthy_count,
            synced_to_cloud: None,
        },
    );

    // Upload health results to cloud if authenticated
    let mut synced = false;
    match check_auth().await {
        Ok(status) if status.authenticated => {
            let client = get_shared_cloud_client();
            match client.upload_health_check(&health_results).await {
                Ok(_) => {
                    tracing::debug!("Health check results synced to cloud");
                    synced = true;
                }
                Err(e) => {
                    tracing::debug!("Failed to upload health check: {}", e);
                }
            }
        }
        Ok(_) => {
            tracing::debug!("Not authenticated, skipping health check cloud sync");
        }
        Err(e) => {
            tracing::debug!("Auth check failed during health upload: {}", e);
        }
    }

    // Emit complete event with actual sync status
    let _ = app.emit(
        HEALTH_CHECK_PROGRESS_EVENT,
        HealthCheckProgress {
            stage: HealthCheckStage::Complete,
            message: format!(
                "Health check complete: {} healthy, {} unreachable",
                healthy_count,
                total - healthy_count
            ),
            total_devices: total,
            checked_devices: total,
            healthy_devices: healthy_count,
            synced_to_cloud: Some(synced),
        },
    );

    // Also emit the legacy event for compatibility
    if let Err(e) = app.emit("health-check-complete", (healthy_count, total)) {
        tracing::debug!("Failed to emit health-check-complete event: {}", e);
    }

    // Mark health check as complete
    HEALTH_CHECK_IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// Run initial scan sequence: full scan followed by immediate health check
async fn run_initial_scan_sequence(app: &AppHandle) {
    tracing::info!("Running initial connection scan sequence");

    // Run full network scan (emits scan-progress events)
    run_scan_and_upload(app).await;

    // Immediately run health check after scan completes
    tracing::info!("Full scan complete, starting health check");
    run_health_checks_with_progress(app).await;
}

pub async fn start_background_scanning(app: AppHandle) {
    // Prevent starting multiple times
    if BACKGROUND_RUNNING.swap(true, Ordering::SeqCst) {
        tracing::debug!("Background scanning already running");
        return;
    }

    tracing::info!("Starting background scanning");

    // Create a cancellation token for graceful shutdown
    let cancel_token = CancellationToken::new();
    {
        let mut token_guard = get_cancel_token_store().lock().await;
        *token_guard = Some(cancel_token.clone());
    }

    let app_scan = app.clone();
    let scan_cancel_token = cancel_token.clone();

    // Spawn background scan task
    tokio::spawn(async move {
        // Run initial scan sequence (full scan + health check)
        run_initial_scan_sequence(&app_scan).await;

        // Then run on interval
        let mut last_interval = SCAN_INTERVAL.load(Ordering::Relaxed);
        let mut scan_timer = interval(Duration::from_secs(last_interval));
        // Skip the first immediate tick since we just ran
        scan_timer.tick().await;

        loop {
            tokio::select! {
                _ = scan_cancel_token.cancelled() => {
                    tracing::info!("Background scan task cancelled");
                    break;
                }
                _ = scan_timer.tick() => {
                    // Check if interval changed
                    let current_interval = SCAN_INTERVAL.load(Ordering::Relaxed);
                    if current_interval != last_interval {
                        last_interval = current_interval;
                        scan_timer = interval(Duration::from_secs(current_interval));
                        scan_timer.tick().await; // Skip immediate tick
                        continue;
                    }

                    run_scan_and_upload(&app_scan).await;
                }
            }
        }
    });

    // Spawn background health check task
    let app_health = app.clone();
    let health_cancel_token = cancel_token.clone();
    tokio::spawn(async move {
        // Wait for initial scan to complete before starting health checks
        tokio::select! {
            _ = health_cancel_token.cancelled() => {
                tracing::info!("Background health check task cancelled during initial wait");
                return;
            }
            _ = tokio::time::sleep(Duration::from_secs(10)) => {}
        }

        let mut last_interval = HEALTH_CHECK_INTERVAL.load(Ordering::Relaxed);
        let mut health_timer = interval(Duration::from_secs(last_interval));
        // Skip the first immediate tick
        health_timer.tick().await;

        loop {
            tokio::select! {
                _ = health_cancel_token.cancelled() => {
                    tracing::info!("Background health check task cancelled");
                    break;
                }
                _ = health_timer.tick() => {
                    // Check if interval changed
                    let current_interval = HEALTH_CHECK_INTERVAL.load(Ordering::Relaxed);
                    if current_interval != last_interval {
                        last_interval = current_interval;
                        health_timer = interval(Duration::from_secs(current_interval));
                        health_timer.tick().await; // Skip immediate tick
                        continue;
                    }

                    run_health_checks_with_progress(&app_health).await;
                }
            }
        }
    });

    // Spawn cloud command poll loop
    let app_commands = app.clone();
    let command_cancel_token = cancel_token.clone();
    tokio::spawn(async move {
        // Wait a few seconds for initial scan to start before polling for commands
        tokio::select! {
            _ = command_cancel_token.cancelled() => {
                tracing::info!("Command poll task cancelled during initial wait");
                return;
            }
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
        }

        run_command_poll_loop(app_commands, command_cancel_token).await;
    });
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceHealthResult {
    pub ip: String,
    pub reachable: bool,
    pub response_time_ms: Option<f64>,
}

// =============================================================================
// Cloud command poll loop
// =============================================================================

/// Event name for cloud command progress updates
pub const CLOUD_COMMAND_EVENT: &str = "cloud-command";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudCommandEvent {
    pub stage: CloudCommandStage,
    pub command_id: i64,
    pub command_type: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudCommandStage {
    Received,
    Executing,
    Completed,
    Failed,
}

/// Long-poll loop that receives commands from the cloud and executes them.
///
/// - Waits for credentials; retries every 10s if not authenticated.
/// - Uses 30s long-poll; the server responds immediately on new commands.
/// - Exponential backoff on errors (1s -> 2s -> 4s -> ... -> 30s max), resets on success.
async fn run_command_poll_loop(app: AppHandle, cancel_token: CancellationToken) {
    let client = get_shared_cloud_client();
    let mut backoff_secs: u64 = 1;
    const MAX_BACKOFF: u64 = 30;
    const POLL_TIMEOUT: u64 = 30;

    loop {
        if cancel_token.is_cancelled() {
            tracing::info!("Command poll loop cancelled");
            return;
        }

        // Load credentials
        let creds = match crate::auth::load_credentials().await {
            Ok(Some(c)) => c,
            _ => {
                tracing::debug!("Not authenticated, waiting before retrying command poll");
                tokio::select! {
                    _ = cancel_token.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(10)) => continue,
                }
            }
        };

        // Long-poll for commands
        let poll_result = tokio::select! {
            _ = cancel_token.cancelled() => return,
            r = client.poll_commands(&creds.access_token, POLL_TIMEOUT) => r,
        };

        match poll_result {
            Ok(poll_response) => {
                backoff_secs = 1; // Reset backoff on success

                for pending_cmd in poll_response.commands {
                    let cmd_id = pending_cmd.id;
                    let cmd_type = pending_cmd.command_type.clone();

                    tracing::info!(
                        "Received cloud command #{}: {}",
                        cmd_id, cmd_type
                    );

                    // Emit received event
                    let _ = app.emit(
                        CLOUD_COMMAND_EVENT,
                        CloudCommandEvent {
                            stage: CloudCommandStage::Received,
                            command_id: cmd_id,
                            command_type: cmd_type.clone(),
                            message: format!("Received command: {}", cmd_type),
                        },
                    );

                    // Claim the command
                    if let Err(e) = client.claim_command(&creds.access_token, cmd_id).await {
                        tracing::warn!("Failed to claim command #{}: {}", cmd_id, e);
                        continue;
                    }

                    // Emit executing event
                    let _ = app.emit(
                        CLOUD_COMMAND_EVENT,
                        CloudCommandEvent {
                            stage: CloudCommandStage::Executing,
                            command_id: cmd_id,
                            command_type: cmd_type.clone(),
                            message: format!("Executing: {}", cmd_type),
                        },
                    );

                    // Execute the command
                    let exec_result = execute_cloud_command(&app, &cmd_type).await;

                    // Report result
                    let report = match &exec_result {
                        Ok(msg) => ResultReport {
                            success: true,
                            result: Some(msg.clone()),
                            error_message: None,
                        },
                        Err(e) => ResultReport {
                            success: false,
                            result: None,
                            error_message: Some(e.to_string()),
                        },
                    };

                    if let Err(e) = client
                        .report_command_result(&creds.access_token, cmd_id, &report)
                        .await
                    {
                        tracing::warn!("Failed to report result for command #{}: {}", cmd_id, e);
                    }

                    // Emit final event
                    let (stage, message) = match &exec_result {
                        Ok(msg) => (CloudCommandStage::Completed, msg.clone()),
                        Err(e) => (CloudCommandStage::Failed, e.to_string()),
                    };

                    let _ = app.emit(
                        CLOUD_COMMAND_EVENT,
                        CloudCommandEvent {
                            stage,
                            command_id: cmd_id,
                            command_type: cmd_type,
                            message,
                        },
                    );
                }
            }
            Err(e) => {
                tracing::debug!(
                    "Command poll error (backoff {}s): {}",
                    backoff_secs, e
                );
                tokio::select! {
                    _ = cancel_token.cancelled() => return,
                    _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {},
                }
                backoff_secs = (backoff_secs * 2).min(MAX_BACKOFF);
            }
        }
    }
}

/// Dispatch a cloud command to the appropriate local action.
async fn execute_cloud_command(app: &AppHandle, command_type: &str) -> Result<String, String> {
    match command_type {
        "scan_network" => {
            if is_scanning() {
                return Err("A scan is already in progress".to_string());
            }
            run_scan_and_upload(app).await;
            let devices = get_known_devices().await;
            Ok(format!("Scan completed: {} devices found", devices.len()))
        }
        "health_check" => {
            if is_health_checking() {
                return Err("A health check is already in progress".to_string());
            }
            run_health_checks_with_progress(app).await;
            let devices = get_known_devices().await;
            let healthy = devices.iter().filter(|d| d.response_time_ms.is_some()).count();
            Ok(format!(
                "Health check completed: {}/{} healthy",
                healthy,
                devices.len()
            ))
        }
        _ => Err(format!("Unknown command type: {}", command_type)),
    }
}

