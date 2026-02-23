use crate::scanner::{Device, ScanResult};
use super::commands::{ClaimResponse, PollResponse, ResultReport, ResultResponse};
use super::config::{load_cloud_config, CloudEndpointConfig};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct CloudClient {
    config: CloudEndpointConfig,
    /// Shared HTTP client for connection pooling - avoids creating new connections per request
    http_client: Arc<reqwest::Client>,
}

impl CloudClient {
    /// Create a new CloudClient with configuration loaded from:
    /// 1. Environment variable (CARTOGRAPHER_CLOUD_URL)
    /// 2. Config file (~/.config/cartographer/config.toml)
    /// 3. Default values (https://cartographer.network/api)
    pub fn new() -> Self {
        let config = load_cloud_config();
        tracing::debug!(
            "CloudClient initialized with {} endpoint: {}",
            config.source,
            config.api_url
        );
        // Create a single HTTP client with connection pooling for reuse across all requests
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .pool_max_idle_per_host(5)
                .build()
                .expect("Failed to create HTTP client")
        );
        Self { config, http_client }
    }

    /// Create a CloudClient with a custom configuration
    pub fn with_config(config: CloudEndpointConfig) -> Self {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .pool_max_idle_per_host(5)
                .build()
                .expect("Failed to create HTTP client")
        );
        Self { config, http_client }
    }

    /// Get the base API URL
    pub fn base_url(&self) -> &str {
        &self.config.api_url
    }

    /// Get the dashboard URL
    pub fn dashboard_url(&self) -> &str {
        &self.config.dashboard_url
    }

    pub async fn request_device_code(&self) -> Result<DeviceCodeResponse> {
        let url = format!("{}/agent/device-code", self.config.api_url);

        let resp = self.http_client
            .post(&url)
            .send()
            .await
            .context("Failed to request device code")?;
        
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Server returned error: {}", resp.status()));
        }
        
        resp.json::<DeviceCodeResponse>()
            .await
            .context("Failed to parse device code response")
    }

    pub async fn poll_for_token(&self, device_code: &str) -> Result<Option<TokenResponse>> {
        let url = format!("{}/agent/token", self.config.api_url);

        let resp = self.http_client
            .post(&url)
            .json(&TokenRequest {
                device_code: device_code.to_string(),
                grant_type: "device_code".to_string(),
            })
            .send()
            .await
            .context("Failed to poll for token")?;
        
        match resp.status().as_u16() {
            200 => {
                let token_resp = resp.json::<TokenResponse>()
                    .await
                    .context("Failed to parse token response")?;
                Ok(Some(token_resp))
            }
            400 => {
                // Still waiting (authorization_pending) or other error
                // Check the error type in the response
                let error_resp: Result<TokenErrorResponse, _> = resp.json().await;
                if let Ok(err) = error_resp {
                    if err.error == "authorization_pending" {
                        return Ok(None);
                    }
                    return Err(anyhow::anyhow!("{}: {}", err.error, err.error_description.unwrap_or_default()));
                }
                Ok(None)
            }
            _ => {
                Err(anyhow::anyhow!("Server returned error: {}", resp.status()))
            }
        }
    }

    /// Result of token verification
    /// - Ok(TokenVerifyResult::Valid) - token is valid
    /// - Ok(TokenVerifyResult::Invalid) - token was rejected by server (401/403)
    /// - Ok(TokenVerifyResult::NetworkError) - couldn't reach server
    pub async fn verify_token(&self, token: &str) -> Result<TokenVerifyResult> {
        let url = format!("{}/agent/verify", self.config.api_url);

        // Use shorter timeout for verification requests
        let resp = match self.http_client
            .get(&url)
            .bearer_auth(token)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                // Network error - could not reach server
                tracing::debug!("Token verification network error: {}", e);
                return Ok(TokenVerifyResult::NetworkError(e.to_string()));
            }
        };

        match resp.status().as_u16() {
            200 => Ok(TokenVerifyResult::Valid),
            401 | 403 => Ok(TokenVerifyResult::Invalid),
            status => {
                // Treat other errors (500, etc.) as transient network issues
                tracing::debug!("Token verification returned status {}", status);
                Ok(TokenVerifyResult::NetworkError(format!("Server returned {}", status)))
            }
        }
    }

    /// Upload scan results to the cloud, including gateway detection and network info.
    pub async fn upload_scan_result(&self, scan_result: &ScanResult) -> Result<()> {
        // Get credentials
        let creds = crate::auth::load_credentials()
            .await
            .context("Failed to load credentials")?
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let url = format!("{}/agent/sync", self.config.api_url);

        let gateway_ip = scan_result.network_info.gateway_ip.as_deref();

        tracing::info!(
            "Uploading {} devices to cloud (network: {}, gateway: {:?})",
            scan_result.devices.len(),
            creds.network_name,
            gateway_ip
        );

        let payload = SyncRequest {
            timestamp: chrono::Utc::now().to_rfc3339(),
            scan_duration_ms: None,
            devices: scan_result
                .devices
                .iter()
                .map(|d| ScanDevice {
                    ip: d.ip.clone(),
                    mac: d.mac.clone(),
                    response_time_ms: d.response_time_ms,
                    hostname: d.hostname.clone(),
                    // Mark device as gateway if its IP matches the detected gateway
                    is_gateway: gateway_ip.map_or(false, |gw| gw == d.ip),
                    vendor: d.vendor.clone(),
                    device_type: d.device_type.clone(),
                })
                .collect(),
            network_info: Some(NetworkInfo {
                subnet: Some(scan_result.network_info.subnet.clone()),
                interface: Some(scan_result.network_info.interface.clone()),
            }),
        };

        let resp = self.http_client
            .post(&url)
            .bearer_auth(&creds.access_token)
            .json(&payload)
            .send()
            .await
            .context("Failed to upload scan")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("Sync failed: {} - {}", status, body);
            return Err(anyhow::anyhow!("Server returned error: {} - {}", status, body));
        }

        tracing::info!("Scan uploaded successfully");
        Ok(())
    }

    /// Legacy function - upload devices without network info (for backward compatibility)
    pub async fn upload_scan(&self, devices: &[Device]) -> Result<()> {
        // Get credentials
        let creds = crate::auth::load_credentials()
            .await
            .context("Failed to load credentials")?
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let url = format!("{}/agent/sync", self.config.api_url);

        tracing::info!(
            "Uploading {} devices to cloud (network: {})",
            devices.len(),
            creds.network_name
        );

        let payload = SyncRequest {
            timestamp: chrono::Utc::now().to_rfc3339(),
            scan_duration_ms: None,
            devices: devices
                .iter()
                .map(|d| ScanDevice {
                    ip: d.ip.clone(),
                    mac: d.mac.clone(),
                    response_time_ms: d.response_time_ms,
                    hostname: d.hostname.clone(),
                    is_gateway: false,
                    vendor: d.vendor.clone(),
                    device_type: d.device_type.clone(),
                })
                .collect(),
            network_info: None,
        };

        let resp = self.http_client
            .post(&url)
            .bearer_auth(&creds.access_token)
            .json(&payload)
            .send()
            .await
            .context("Failed to upload scan")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            tracing::error!("Sync failed: {} - {}", status, body);
            return Err(anyhow::anyhow!("Server returned error: {} - {}", status, body));
        }

        tracing::info!("Scan uploaded successfully");
        Ok(())
    }

    pub async fn get_network_info(&self) -> Result<NetworkInfoResponse> {
        let creds = crate::auth::load_credentials().await
            .context("Failed to load credentials")?
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let url = format!("{}/agent/network", self.config.api_url);

        let resp = self.http_client
            .get(&url)
            .bearer_auth(&creds.access_token)
            .send()
            .await
            .context("Failed to get network info")?;
        
        if !resp.status().is_success() {
            return Err(anyhow::anyhow!("Server returned error: {}", resp.status()));
        }
        
        resp.json::<NetworkInfoResponse>()
            .await
            .context("Failed to parse network info response")
    }

    pub async fn upload_health_check(&self, results: &[crate::scheduler::DeviceHealthResult]) -> Result<()> {
        let creds = crate::auth::load_credentials().await
            .context("Failed to load credentials")?
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        let url = format!("{}/agent/health", self.config.api_url);

        let payload = HealthCheckRequest {
            timestamp: chrono::Utc::now().to_rfc3339(),
            results: results.iter().map(|r| HealthCheckResult {
                ip: r.ip.clone(),
                reachable: r.reachable,
                response_time_ms: r.response_time_ms,
            }).collect(),
        };

        let resp = self.http_client
            .post(&url)
            .bearer_auth(&creds.access_token)
            .json(&payload)
            .send()
            .await
            .context("Failed to upload health check")?;
        
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Server returned error: {} - {}", status, body));
        }
        
        Ok(())
    }

    pub async fn open_dashboard(&self) -> Result<()> {
        let creds = crate::auth::load_credentials().await
            .context("Failed to load credentials")?
            .ok_or_else(|| anyhow::anyhow!("Not authenticated"))?;

        // Navigate to the core app with network context (using configurable dashboard URL)
        let url = format!("{}/app/network/{}", self.config.dashboard_url, creds.network_id);
        webbrowser::open(&url)
            .context("Failed to open dashboard in browser")
    }

    /// Long-poll for pending commands. Uses a client timeout of `timeout_secs + 5`
    /// to give the server time to respond before the client gives up.
    pub async fn poll_commands(&self, token: &str, timeout_secs: u64) -> Result<PollResponse> {
        let url = format!(
            "{}/agent/commands/poll?timeout={}",
            self.config.api_url, timeout_secs
        );

        let resp = self.http_client
            .get(&url)
            .bearer_auth(token)
            .timeout(std::time::Duration::from_secs(timeout_secs + 5))
            .send()
            .await
            .context("Failed to poll for commands")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Poll failed: {} - {}", status, body));
        }

        resp.json::<PollResponse>()
            .await
            .context("Failed to parse poll response")
    }

    /// Claim a pending command so no other agent instance picks it up.
    pub async fn claim_command(&self, token: &str, command_id: i64) -> Result<ClaimResponse> {
        let url = format!(
            "{}/agent/commands/{}/claim",
            self.config.api_url, command_id
        );

        let resp = self.http_client
            .post(&url)
            .bearer_auth(token)
            .send()
            .await
            .context("Failed to claim command")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Claim failed: {} - {}", status, body));
        }

        resp.json::<ClaimResponse>()
            .await
            .context("Failed to parse claim response")
    }

    /// Report the result of a command execution back to the cloud.
    pub async fn report_command_result(
        &self,
        token: &str,
        command_id: i64,
        report: &ResultReport,
    ) -> Result<ResultResponse> {
        let url = format!(
            "{}/agent/commands/{}/result",
            self.config.api_url, command_id
        );

        let resp = self.http_client
            .post(&url)
            .bearer_auth(token)
            .json(report)
            .send()
            .await
            .context("Failed to report command result")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Report failed: {} - {}", status, body));
        }

        resp.json::<ResultResponse>()
            .await
            .context("Failed to parse result response")
    }
}

/// Result of token verification attempt
#[derive(Debug, Clone)]
pub enum TokenVerifyResult {
    /// Token is valid
    Valid,
    /// Token was explicitly rejected by the server (401/403)
    Invalid,
    /// Could not reach the server (network error, timeout, server error)
    NetworkError(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenRequest {
    device_code: String,
    grant_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenErrorResponse {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub network_id: String,
    pub network_name: String,
    pub user_email: String,
    pub automatic_full_scan_min_interval_seconds: Option<u64>,
}

#[derive(Debug, Serialize)]
struct SyncRequest {
    timestamp: String,
    scan_duration_ms: Option<u64>,
    devices: Vec<ScanDevice>,
    network_info: Option<NetworkInfo>,
}

#[derive(Debug, Serialize)]
struct NetworkInfo {
    subnet: Option<String>,
    interface: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanDevice {
    ip: String,
    mac: Option<String>,
    response_time_ms: Option<f64>,
    hostname: Option<String>,
    is_gateway: bool,
    /// Device vendor/manufacturer from MAC OUI lookup
    vendor: Option<String>,
    /// Inferred device type based on vendor
    device_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NetworkInfoResponse {
    pub network_id: String,
    pub network_name: String,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct HealthCheckRequest {
    timestamp: String,
    results: Vec<HealthCheckResult>,
}

#[derive(Debug, Serialize)]
struct HealthCheckResult {
    ip: String,
    reachable: bool,
    response_time_ms: Option<f64>,
}
