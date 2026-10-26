use axum::{
    Extension, Json, Router, extract::State, middleware::from_fn_with_state,
    response::Json as ResponseJson, routing::get,
};
use db::models::{
    execution_process::{ExecutionProcess, ExecutionProcessRunReason, ExecutionProcessStatus},
    scratch::DraftFollowUpData,
    session::{Session, SessionError},
};
use deployment::Deployment;
use executors::{
    executors::{BaseCodingAgent, CodingAgent},
    profile::ExecutorProfileId,
};
use serde::Deserialize;
use services::services::queued_message::QueueStatus;
use ts_rs::TS;
use utils::response::ApiResponse;

use crate::{DeploymentImpl, error::ApiError, middleware::load_session_middleware};

/// Request body for queueing a follow-up message
#[derive(Debug, Deserialize, TS)]
pub struct QueueMessageRequest {
    pub message: String,
    pub executor_profile_id: ExecutorProfileId,
    #[serde(default)]
    #[ts(optional)]
    pub executor_config: Option<Box<CodingAgent>>,
}

/// Queue a follow-up message to be executed when the current execution finishes
pub async fn queue_message(
    Extension(session): Extension<Session>,
    State(deployment): State<DeploymentImpl>,
    Json(payload): Json<QueueMessageRequest>,
) -> Result<ResponseJson<ApiResponse<QueueStatus>>, ApiError> {
    let pool = &deployment.db().pool;

    // A queued message is only consumed when a running execution finishes, so
    // refuse to queue when no CodingAgent execution is running for this
    // session; otherwise the message would be stranded forever.
    let has_running_execution = ExecutionProcess::find_by_session_id(pool, session.id, false)
        .await?
        .iter()
        .any(|process| {
            process.status == ExecutionProcessStatus::Running
                && process.run_reason == ExecutionProcessRunReason::CodingAgent
        });
    if !has_running_execution {
        return Err(ApiError::Conflict(
            "No running execution for this session; cannot queue a follow-up message".to_string(),
        ));
    }

    if payload.executor_config.as_ref().is_some_and(|config| {
        BaseCodingAgent::from(config.as_ref()) != payload.executor_profile_id.executor
    }) {
        return Err(ApiError::BadRequest(
            "Executor override does not match the executor profile".to_string(),
        ));
    }

    // Validate executor matches session if session has prior executions
    let expected_executor: Option<String> =
        ExecutionProcess::latest_executor_profile_for_session(pool, session.id)
            .await?
            .map(|profile| profile.executor.to_string())
            .or_else(|| session.executor.clone());

    if let Some(expected) = expected_executor {
        let actual = payload.executor_profile_id.executor.to_string();
        if expected != actual {
            return Err(ApiError::Session(SessionError::ExecutorMismatch {
                expected,
                actual,
            }));
        }
    }

    let data = DraftFollowUpData {
        message: payload.message,
        executor_profile_id: payload.executor_profile_id,
        executor_config: payload.executor_config,
    };

    let queued = deployment
        .queued_message_service()
        .queue_message(session.id, data);

    deployment
        .track_if_analytics_allowed(
            "follow_up_queued",
            serde_json::json!({
                "session_id": session.id.to_string(),
                "workspace_id": session.workspace_id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(QueueStatus::Queued {
        message: queued,
    })))
}

/// Cancel a queued follow-up message
pub async fn cancel_queued_message(
    Extension(session): Extension<Session>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<QueueStatus>>, ApiError> {
    deployment
        .queued_message_service()
        .cancel_queued(session.id);

    deployment
        .track_if_analytics_allowed(
            "follow_up_queue_cancelled",
            serde_json::json!({
                "session_id": session.id.to_string(),
                "workspace_id": session.workspace_id.to_string(),
            }),
        )
        .await;

    Ok(ResponseJson(ApiResponse::success(QueueStatus::Empty)))
}

/// Get the current queue status for a session's workspace
pub async fn get_queue_status(
    Extension(session): Extension<Session>,
    State(deployment): State<DeploymentImpl>,
) -> Result<ResponseJson<ApiResponse<QueueStatus>>, ApiError> {
    let status = deployment.queued_message_service().get_status(session.id);

    Ok(ResponseJson(ApiResponse::success(status)))
}

pub fn router(deployment: &DeploymentImpl) -> Router<DeploymentImpl> {
    Router::new()
        .route(
            "/",
            get(get_queue_status)
                .post(queue_message)
                .delete(cancel_queued_message),
        )
        .layer(from_fn_with_state(
            deployment.clone(),
            load_session_middleware,
        ))
}
