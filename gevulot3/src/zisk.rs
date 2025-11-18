use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, oneshot};
use tonic::transport::Channel;
use tracing::{error, info, warn};
use zisk_distributed_grpc_api::{InputMode, LaunchProofRequest};
use zisk_distributed_grpc_api::zisk_distributed_api_client::ZiskDistributedApiClient;

/// Webhook payload received from the coordinator when a job completes.
/// See `WebhookPayloadDto`:
/// https://github.com/0xPolygonHermez/zisk/blob/b9acdeecf5601f89824570e4337cb084c4ca501e/distributed/crates/common/src/dto.rs#L207-L217.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
  pub job_id: String,
  pub success: bool,
  pub duration_ms: u64,
  pub proof: Option<Vec<u64>>,
  pub executed_steps: Option<u64>,
  pub timestamp: String,
  pub error: Option<WebhookError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookError {
  pub code: String,
  pub message: String,
}

/// Shared state for the webhook server.
#[derive(Clone)]
struct AppState {
  /// Map of job IDs to their result channels.
  pending_jobs: Arc<Mutex<HashMap<String, oneshot::Sender<WebhookPayload>>>>,
}

/// Webhook endpoint handler - receives proof completion notifications from coordinator.
async fn webhook_handler(
  State(state): State<AppState>,
  Json(payload): Json<WebhookPayload>,
) -> StatusCode {
  info!("Received webhook for job: {}", payload.job_id);

  if payload.success {
    info!(
      "Job completed successfully! Proof size: {} elements",
      payload.proof.as_ref().map(|p| p.len()).unwrap_or(0)
    );
  } else {
    error!("Job failed: {:?}", payload.error);
  }

  // Look up the job in pending jobs and send the result.
  let mut pending_jobs = state.pending_jobs.lock().await;
  match pending_jobs.remove(&payload.job_id) {
    Some(tx) => {
      if tx.send(payload).is_err() {
        error!("Failed to send webhook result to main thread");
      }
    }
    None => {
      warn!("Received webhook for unknown job ID: {}", payload.job_id);
    }
  };

  StatusCode::OK
}

/// Submits a proof job to the coordinator.
pub async fn submit_proof_job(
  coordinator_url: &str,
  input_file_path: &str,
  compute_capacity: u32,
) -> Result<String> {
  info!("Connecting to coordinator at {}", coordinator_url);

  // Connect to coordinator.
  let channel = Channel::from_shared(coordinator_url.to_string())?
    .connect()
    .await
    .context("Failed to connect to coordinator")?;

  // Set max message size to 50MB for sending/receiving large input data.
  let max_message_size = 50 * 1024 * 1024; // 50MB
  let mut client = ZiskDistributedApiClient::new(channel)
    .max_decoding_message_size(max_message_size)
    .max_encoding_message_size(max_message_size);

  // Create launch proof request.
  let data_id = uuid::Uuid::new_v4().to_string();
  let request = LaunchProofRequest {
    compute_capacity,
    data_id,
    input_mode: InputMode::Data.into(),
    input_path: Some(input_file_path.to_string()), // CAUTION: File must be availiable locally on coordinator machine. 
    simulated_node: None,
  };

  info!(
    "Submitting proof job with {} compute units",
    compute_capacity
  );

  // Submit the job.
  let response = client
    .launch_proof(request)
    .await
    .context("Failed to launch proof job")?;

  // Extract job ID from response.
  let job_id = match response.into_inner().result {
    Some(zisk_distributed_grpc_api::launch_proof_response::Result::JobId(job_id)) => {
      info!("Proof job submitted successfully! Job ID: {}", job_id);
      job_id
    }
    Some(zisk_distributed_grpc_api::launch_proof_response::Result::Error(error)) => {
      anyhow::bail!("Job submission failed: {} - {}", error.code, error.message);
    }
    None => {
      anyhow::bail!("Received empty response from coordinator");
    }
  };

  Ok(job_id)
}

/// Handle to a long-running webhook server.
#[derive(Clone)]
pub struct WebhookServer {
  state: AppState,
}

impl WebhookServer {
  /// Registers a job and returns a receiver for the result.
  pub async fn register_job(&self, job_id: String) -> oneshot::Receiver<WebhookPayload> {
    let (tx, rx) = oneshot::channel();
    let mut pending_jobs = self.state.pending_jobs.lock().await;
    pending_jobs.insert(job_id.clone(), tx);
    info!("Registered job {} for webhook callback", job_id);
    rx
  }
}

/// Starts a long-running webhook server and returns a handle to it and the server task.
pub async fn start_webhook_server(
  port: u16,
) -> Result<(WebhookServer, tokio::task::JoinHandle<()>)> {
  let state = AppState {
    pending_jobs: Arc::new(Mutex::new(HashMap::new())),
  };

  let app = Router::new()
    .route("/webhook/{job_id}", post(webhook_handler))
    .with_state(state.clone());

  let addr = format!("0.0.0.0:{}", port);
  info!("Starting webhook server on {}", addr);

  let listener = tokio::net::TcpListener::bind(&addr)
    .await
    .context(format!("Failed to bind to {}", addr))?;

  // Spawn the server in a background task.
  let server_handle = tokio::spawn(async move {
    if let Err(e) = axum::serve(listener, app).await {
      error!("Webhook server error: {}", e);
    }
  });

  Ok((WebhookServer { state }, server_handle))
}
