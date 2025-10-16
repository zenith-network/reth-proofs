use std::sync::Arc;

use anyhow::{Context, Result};
use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tonic::transport::Channel;
use tracing::{error, info};
use zisk_distributed_grpc_api::LaunchProofRequest;
use zisk_distributed_grpc_api::zisk_distributed_api_client::ZiskDistributedApiClient;

/// Webhook payload received from the coordinator when a job completes.
/// See: https://github.com/0xPolygonHermez/zisk/blob/5104c56c4736f99e1a3e809511e41ed6306a7db5/distributed/crates/common/src/dto.rs#L204.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookPayload {
  pub job_id: String,
  pub success: bool,
  pub duration_ms: u64,
  pub proof: Option<Vec<u64>>,
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
  /// Channel to send the proof result back to the main thread.
  result_tx: Arc<tokio::sync::Mutex<Option<oneshot::Sender<WebhookPayload>>>>,
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

  // Send the result to the waiting thread.
  let mut tx_guard = state.result_tx.lock().await;
  if let Some(tx) = tx_guard.take() {
    if tx.send(payload).is_err() {
      error!("Failed to send webhook result to main thread");
    }
  }

  StatusCode::OK
}

/// Submits a proof job to the coordinator.
pub async fn submit_proof_job(
  coordinator_url: &str,
  input_data: Vec<u8>,
  input_file_name: &str,
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
  let request = LaunchProofRequest {
    block_id: "example-block-001".to_string(),
    compute_capacity,
    input_path: input_file_name.to_string(), // Kept for reference only.
    simulated_node: None,
    input_data,
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

/// Starts the webhook server and returns the server handle and result receiver.
pub async fn start_webhook_server(
  port: u16,
) -> Result<(
  tokio::task::JoinHandle<()>,
  oneshot::Receiver<WebhookPayload>,
)> {
  let (result_tx, result_rx) = oneshot::channel();

  let state = AppState {
    result_tx: Arc::new(tokio::sync::Mutex::new(Some(result_tx))),
  };

  let app = Router::new()
    .route("/webhook/{job_id}", post(webhook_handler))
    .with_state(state);

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

  Ok((server_handle, result_rx))
}
