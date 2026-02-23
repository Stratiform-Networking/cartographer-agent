use serde::{Deserialize, Serialize};

/// A command received during long-poll.
#[derive(Debug, Clone, Deserialize)]
pub struct PendingCommand {
    pub id: i64,
    pub command_type: String,
    pub payload: Option<String>,
}

/// Wrapper returned by the poll endpoint.
#[derive(Debug, Deserialize)]
pub struct PollResponse {
    pub commands: Vec<PendingCommand>,
}

/// Confirmation after claiming a command.
#[derive(Debug, Deserialize)]
pub struct ClaimResponse {
    pub id: i64,
    pub command_type: String,
    pub payload: Option<String>,
    pub claimed_at: String,
}

/// Report sent to the cloud after executing a command.
#[derive(Debug, Serialize)]
pub struct ResultReport {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// Confirmation returned after reporting a result.
#[derive(Debug, Deserialize)]
pub struct ResultResponse {
    pub id: i64,
    pub status: String,
    pub completed_at: String,
}
