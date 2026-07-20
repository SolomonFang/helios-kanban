use axum::{
    Router,
    extract::{Path, State},
    response::{IntoResponse, Json as ResponseJson},
    routing::{get, post},
};
use deployment::Deployment;
use services::services::approvals::ApprovalInfo;
use utils::{
    approvals::{ApprovalOutcome, ApprovalResponse},
    log_msg::LogMsg,
    response::ApiResponse,
};

use crate::DeploymentImpl;

pub async fn list_approvals(
    State(deployment): State<DeploymentImpl>,
) -> ResponseJson<ApiResponse<Vec<ApprovalInfo>>> {
    ResponseJson(ApiResponse::success(deployment.approvals().list_pending()))
}

pub async fn respond_to_approval(
    State(deployment): State<DeploymentImpl>,
    Path(id): Path<String>,
    ResponseJson(request): ResponseJson<ApprovalResponse>,
) -> Result<ResponseJson<ApiResponse<ApprovalOutcome>>, axum::http::StatusCode> {
    let service = deployment.approvals();

    match service
        .respond(&deployment.db().pool, &id, request)
        .await
    {
        Ok((outcome, context)) => {
            deployment
                .track_if_analytics_allowed(
                    "approval_responded",
                    serde_json::json!({
                        "approval_id": &id,
                        "status": format!("{:?}", outcome),
                        "tool_name": context.tool_name,
                        "execution_process_id": context.execution_process_id.to_string(),
                    }),
                )
                .await;

            Ok(ResponseJson(ApiResponse::success(outcome)))
        }
        Err(e) => {
            tracing::error!("Failed to respond to approval: {:?}", e);
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub async fn stream_approvals_ws(
    ws: axum::extract::ws::WebSocketUpgrade,
    State(deployment): State<DeploymentImpl>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_approvals_ws(socket, deployment).await {
            tracing::warn!("approvals WS closed: {}", e);
        }
    })
}

async fn handle_approvals_ws(
    socket: axum::extract::ws::WebSocket,
    deployment: DeploymentImpl,
) -> anyhow::Result<()> {
    use futures_util::{SinkExt, StreamExt};

    let mut stream = deployment.approvals().patch_stream();
    let (mut sender, mut receiver) = socket.split();

    tokio::spawn(async move { while receiver.next().await.is_some() {} });

    if let Some(snapshot_patch) = stream.next().await {
        let _ = sender
            .send(LogMsg::JsonPatch(snapshot_patch).to_ws_message_unchecked())
            .await;
    }

    let _ = sender.send(LogMsg::Ready.to_ws_message_unchecked()).await;

    while let Some(patch) = stream.next().await {
        if sender
            .send(LogMsg::JsonPatch(patch).to_ws_message_unchecked())
            .await
            .is_err()
        {
            break;
        }
    }

    Ok(())
}

pub fn router() -> Router<DeploymentImpl> {
    Router::new()
        .route("/approvals", get(list_approvals))
        .route("/approvals/{id}/respond", post(respond_to_approval))
        .route("/approvals/stream/ws", get(stream_approvals_ws))
}
