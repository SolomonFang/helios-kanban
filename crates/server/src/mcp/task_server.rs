use std::{future::Future, str::FromStr};

use db::models::{
    project::Project,
    repo::Repo,
    tag::Tag,
    task::{CreateTask, Task, TaskStatus, TaskWithAttemptStatus, UpdateTask},
    workspace::{Workspace, WorkspaceContext},
};
use executors::{executors::BaseCodingAgent, profile::ExecutorProfileId};
use regex::Regex;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::tool::{Parameters, ToolRouter},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json;
use services::services::approvals::ApprovalInfo;
use utils::approvals::{ApprovalOutcome, ApprovalResponse};
use uuid::Uuid;

use crate::routes::{
    containers::ContainerQuery,
    task_attempts::{
        CreateTaskAttemptBody, WorkspaceRepoInput, workspace_summary::WorkspaceSummaryRequest,
    },
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTaskRequest {
    #[schemars(description = "The ID of the project to create the task in. This is required!")]
    pub project_id: Uuid,
    #[schemars(description = "The title of the task")]
    pub title: String,
    #[schemars(description = "Optional description of the task")]
    pub description: Option<String>,
    #[schemars(
        description = "Optional iteration code to assign the task to (e.g. '260717'). Omit to leave unassigned."
    )]
    pub iteration: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CreateTaskResponse {
    pub task_id: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectSummary {
    #[schemars(description = "The unique identifier of the project")]
    pub id: String,
    #[schemars(description = "The name of the project")]
    pub name: String,
    #[schemars(
        description = "Optional description of what this project is for — use this to decide where to create tasks"
    )]
    pub description: Option<String>,
    #[schemars(description = "Repository names linked to this project (summary)")]
    pub repos: Vec<String>,
    #[schemars(description = "When the project was created")]
    pub created_at: String,
    #[schemars(description = "When the project was last updated")]
    pub updated_at: String,
}

impl ProjectSummary {
    fn from_project_with_repos(project: Project, repos: Vec<String>) -> Self {
        Self {
            id: project.id.to_string(),
            name: project.name,
            description: project.description,
            repos,
            created_at: project.created_at.to_rfc3339(),
            updated_at: project.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetProjectRequest {
    #[schemars(description = "The ID of the project to retrieve")]
    pub project_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GetProjectResponse {
    pub project: ProjectSummary,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateProjectRequest {
    #[schemars(description = "The ID of the project to update")]
    pub project_id: Uuid,
    #[schemars(description = "New project name")]
    pub name: Option<String>,
    #[schemars(
        description = "New project description for humans/agents. Pass empty string to clear. Omit to keep existing."
    )]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct UpdateProjectResponse {
    pub project: ProjectSummary,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct McpRepoSummary {
    #[schemars(description = "The unique identifier of the repository")]
    pub id: String,
    #[schemars(description = "The name of the repository")]
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListReposRequest {
    #[schemars(description = "The ID of the project to list repositories from")]
    pub project_id: Uuid,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetRepoRequest {
    #[schemars(description = "The ID of the repository to retrieve")]
    pub repo_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RepoDetails {
    #[schemars(description = "The unique identifier of the repository")]
    pub id: String,
    #[schemars(description = "The name of the repository")]
    pub name: String,
    #[schemars(description = "The display name of the repository")]
    pub display_name: String,
    #[schemars(description = "The setup script that runs when initializing a workspace")]
    pub setup_script: Option<String>,
    #[schemars(description = "The cleanup script that runs when tearing down a workspace")]
    pub cleanup_script: Option<String>,
    #[schemars(description = "The dev server script that starts the development server")]
    pub dev_server_script: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateSetupScriptRequest {
    #[schemars(description = "The ID of the repository to update")]
    pub repo_id: Uuid,
    #[schemars(description = "The new setup script content (use empty string to clear)")]
    pub script: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateCleanupScriptRequest {
    #[schemars(description = "The ID of the repository to update")]
    pub repo_id: Uuid,
    #[schemars(description = "The new cleanup script content (use empty string to clear)")]
    pub script: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateDevServerScriptRequest {
    #[schemars(description = "The ID of the repository to update")]
    pub repo_id: Uuid,
    #[schemars(description = "The new dev server script content (use empty string to clear)")]
    pub script: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct UpdateRepoScriptResponse {
    #[schemars(description = "Whether the update was successful")]
    pub success: bool,
    #[schemars(description = "The repository ID that was updated")]
    pub repo_id: String,
    #[schemars(description = "The script field that was updated")]
    pub field: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListReposResponse {
    pub repos: Vec<McpRepoSummary>,
    pub count: usize,
    pub project_id: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListProjectsResponse {
    pub projects: Vec<ProjectSummary>,
    pub count: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListTasksRequest {
    #[schemars(description = "The ID of the project to list tasks from")]
    pub project_id: Uuid,
    #[schemars(
        description = "Optional status filter: 'todo', 'inprogress', 'inreview', 'done', 'cancelled'"
    )]
    pub status: Option<String>,
    #[schemars(description = "Optional iteration code filter (e.g. '260717')")]
    pub iteration: Option<String>,
    #[schemars(description = "Optional case-insensitive search in title/description")]
    pub query: Option<String>,
    #[schemars(description = "Maximum number of tasks to return (default: 50)")]
    pub limit: Option<i32>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskSummary {
    #[schemars(description = "The unique identifier of the task")]
    pub id: String,
    #[schemars(description = "The title of the task")]
    pub title: String,
    #[schemars(description = "Current status of the task")]
    pub status: String,
    #[schemars(description = "Iteration code assigned to the task, if any")]
    pub iteration: Option<String>,
    #[schemars(description = "When the task was created")]
    pub created_at: String,
    #[schemars(description = "When the task was last updated")]
    pub updated_at: String,
    #[schemars(description = "Whether the task has an in-progress execution attempt")]
    pub has_in_progress_attempt: Option<bool>,
    #[schemars(description = "Whether the last execution attempt failed")]
    pub last_attempt_failed: Option<bool>,
}

impl TaskSummary {
    fn from_task_with_status(task: TaskWithAttemptStatus) -> Self {
        Self {
            id: task.id.to_string(),
            title: task.title.to_string(),
            status: task.status.to_string(),
            iteration: task.iteration.clone(),
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
            has_in_progress_attempt: Some(task.has_in_progress_attempt),
            last_attempt_failed: Some(task.last_attempt_failed),
        }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskDetails {
    #[schemars(description = "The unique identifier of the task")]
    pub id: String,
    #[schemars(description = "The title of the task")]
    pub title: String,
    #[schemars(description = "Optional description of the task")]
    pub description: Option<String>,
    #[schemars(description = "Current status of the task")]
    pub status: String,
    #[schemars(description = "Iteration code assigned to the task, if any")]
    pub iteration: Option<String>,
    #[schemars(description = "When the task was created")]
    pub created_at: String,
    #[schemars(description = "When the task was last updated")]
    pub updated_at: String,
    #[schemars(description = "Whether the task has an in-progress execution attempt")]
    pub has_in_progress_attempt: Option<bool>,
    #[schemars(description = "Whether the last execution attempt failed")]
    pub last_attempt_failed: Option<bool>,
}

impl TaskDetails {
    fn from_task(task: Task) -> Self {
        Self {
            id: task.id.to_string(),
            title: task.title,
            description: task.description,
            status: task.status.to_string(),
            iteration: task.iteration,
            created_at: task.created_at.to_rfc3339(),
            updated_at: task.updated_at.to_rfc3339(),
            has_in_progress_attempt: None,
            last_attempt_failed: None,
        }
    }
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListTasksResponse {
    pub tasks: Vec<TaskSummary>,
    pub count: usize,
    pub project_id: String,
    pub applied_filters: ListTasksFilters,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListTasksFilters {
    pub status: Option<String>,
    pub iteration: Option<String>,
    pub query: Option<String>,
    pub limit: i32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpdateTaskRequest {
    #[schemars(description = "The ID of the task to update")]
    pub task_id: Uuid,
    #[schemars(description = "New title for the task")]
    pub title: Option<String>,
    #[schemars(description = "New description for the task")]
    pub description: Option<String>,
    #[schemars(description = "New status: 'todo', 'inprogress', 'inreview', 'done', 'cancelled'")]
    pub status: Option<String>,
    #[schemars(
        description = "New iteration code (e.g. '260717'). Pass empty string to clear. Omit to keep existing."
    )]
    pub iteration: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct UpdateTaskResponse {
    pub task: TaskDetails,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DeleteTaskRequest {
    #[schemars(description = "The ID of the task to delete")]
    pub task_id: Uuid,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McpWorkspaceRepoInput {
    #[schemars(description = "The repository ID")]
    pub repo_id: Uuid,
    #[schemars(
        description = "Optional base/target branch. Omit to use the repo's default_target_branch. If that is also unset, the call fails (no silent fallback to main)."
    )]
    pub base_branch: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StartWorkspaceSessionRequest {
    #[schemars(description = "The ID of the task to start")]
    pub task_id: Uuid,
    #[schemars(
        description = "Optional coding agent executor ('CLAUDE_CODE', 'AMP', 'GEMINI', 'CODEX', 'OPENCODE', 'CURSOR_AGENT', 'QWEN_CODE', 'COPILOT', 'DROID', 'REASONIX', 'KIMI_CLI'). Omit to use Settings default from /api/info → config.executor_profile."
    )]
    pub executor: Option<String>,
    #[schemars(description = "Optional executor variant, if needed")]
    pub variant: Option<String>,
    #[schemars(
        description = "Repositories to include. Pass at least one {repo_id}; base_branch is optional."
    )]
    pub repos: Vec<McpWorkspaceRepoInput>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StartWorkspaceSessionResponse {
    pub task_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StopWorkspaceSessionRequest {
    #[schemars(description = "The workspace ID to stop")]
    pub workspace_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StopWorkspaceSessionResponse {
    pub workspace_id: String,
    pub stopped: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CancelTaskRequest {
    #[schemars(description = "The ID of the task to soft-cancel (status=cancelled)")]
    pub task_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CancelTaskResponse {
    pub task: TaskDetails,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DeleteTaskResponse {
    pub deleted_task_id: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTaskRequest {
    #[schemars(description = "The ID of the task to retrieve")]
    pub task_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GetTaskResponse {
    pub task: TaskDetails,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FollowUpSessionRequest {
    #[schemars(
        description = "The ID of the session to send the follow-up to. Use get_task_status to find a workspace's latest_session_id."
    )]
    pub session_id: Uuid,
    #[schemars(description = "The follow-up instruction/prompt for the agent")]
    pub prompt: String,
    #[schemars(
        description = "Optional coding agent executor ('CLAUDE_CODE', 'AMP', 'GEMINI', 'CODEX', 'OPENCODE', 'CURSOR_AGENT', 'QWEN_CODE', 'COPILOT', 'DROID', 'REASONIX', 'KIMI_CLI'). Omit to reuse the session's executor (falls back to Settings default)."
    )]
    pub executor: Option<String>,
    #[schemars(description = "Optional executor variant, if needed")]
    pub variant: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct FollowUpSessionResponse {
    pub session_id: String,
    pub execution_process_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QueueMessageRequest {
    #[schemars(
        description = "The ID of the session to queue the message for. Use get_task_status to find a workspace's latest_session_id."
    )]
    pub session_id: Uuid,
    #[schemars(
        description = "The message to deliver to the agent when its current execution finishes"
    )]
    pub message: String,
    #[schemars(
        description = "Optional coding agent executor. Omit to reuse the session's executor (falls back to Settings default)."
    )]
    pub executor: Option<String>,
    #[schemars(description = "Optional executor variant, if needed")]
    pub variant: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct QueueMessageResponse {
    pub session_id: String,
    pub queued: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ApprovalSummary {
    #[schemars(description = "The approval ID, to pass to respond_to_approval")]
    pub approval_id: String,
    #[schemars(description = "Name of the tool requesting approval")]
    pub tool_name: String,
    #[schemars(description = "The execution process waiting on this approval")]
    pub execution_process_id: String,
    #[schemars(
        description = "True if this is a question from the agent (cannot be answered via respond_to_approval)"
    )]
    pub is_question: bool,
    #[schemars(description = "When the approval was requested")]
    pub created_at: String,
    #[schemars(description = "When the approval request times out")]
    pub timeout_at: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListApprovalsResponse {
    pub approvals: Vec<ApprovalSummary>,
    pub count: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct RespondToApprovalRequest {
    #[schemars(description = "The approval ID from list_approvals")]
    pub approval_id: String,
    #[schemars(description = "Decision: 'approved' or 'denied'")]
    pub decision: String,
    #[schemars(description = "Optional reason, used when denying")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RespondToApprovalResponse {
    pub approval_id: String,
    pub outcome: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateTaskAndStartRequest {
    #[schemars(description = "The ID of the project to create the task in. This is required!")]
    pub project_id: Uuid,
    #[schemars(description = "The title of the task")]
    pub title: String,
    #[schemars(description = "Optional description of the task")]
    pub description: Option<String>,
    #[schemars(
        description = "Optional iteration code to assign the task to (e.g. '260717'). Omit to leave unassigned."
    )]
    pub iteration: Option<String>,
    #[schemars(
        description = "Optional coding agent executor. Omit to use Settings default from /api/info → config.executor_profile."
    )]
    pub executor: Option<String>,
    #[schemars(description = "Optional executor variant, if needed")]
    pub variant: Option<String>,
    #[schemars(
        description = "Repositories to include. Pass at least one {repo_id}; base_branch is optional."
    )]
    pub repos: Vec<McpWorkspaceRepoInput>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CreateTaskAndStartResponse {
    pub task_id: String,
    pub workspace_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTaskStatusRequest {
    #[schemars(description = "The ID of the task to get the status for")]
    pub task_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaskWorkspaceStatus {
    #[schemars(description = "The unique identifier of the workspace")]
    pub workspace_id: String,
    #[schemars(description = "Optional display name of the workspace")]
    pub name: Option<String>,
    #[schemars(description = "The workspace branch")]
    pub branch: String,
    #[schemars(description = "Whether the workspace is archived")]
    pub archived: bool,
    #[schemars(description = "When the workspace was created")]
    pub created_at: String,
    #[schemars(
        description = "Session ID of the latest execution process (use for follow_up_session / queue_message)"
    )]
    pub latest_session_id: Option<String>,
    #[schemars(
        description = "Status of the latest execution process: 'running', 'completed', 'failed', 'killed'"
    )]
    pub latest_process_status: Option<String>,
    #[schemars(description = "When the latest execution process completed")]
    pub latest_process_completed_at: Option<String>,
    #[schemars(description = "Is a tool approval currently pending?")]
    pub has_pending_approval: bool,
    #[schemars(description = "Is a dev server currently running?")]
    pub has_running_dev_server: bool,
    #[schemars(description = "Does this workspace have unseen coding agent turns?")]
    pub has_unseen_turns: bool,
    #[schemars(description = "PR status for this workspace: 'open', 'merged', 'closed'")]
    pub pr_status: Option<String>,
    #[schemars(description = "Number of files with changes")]
    pub files_changed: Option<usize>,
    #[schemars(description = "Total lines added across all files")]
    pub lines_added: Option<usize>,
    #[schemars(description = "Total lines removed across all files")]
    pub lines_removed: Option<usize>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GetTaskStatusResponse {
    pub task: TaskDetails,
    pub workspaces: Vec<TaskWorkspaceStatus>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListBranchesRequest {
    #[schemars(description = "The ID of the repository to list branches from")]
    pub repo_id: Uuid,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BranchSummary {
    #[schemars(description = "The branch name")]
    pub name: String,
    #[schemars(description = "Whether this is the currently checked-out branch")]
    pub is_current: bool,
    #[schemars(description = "Whether this is a remote-tracking branch")]
    pub is_remote: bool,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListBranchesResponse {
    pub repo_id: String,
    pub branches: Vec<BranchSummary>,
    pub count: usize,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TagSummary {
    #[schemars(description = "The unique identifier of the tag")]
    pub id: String,
    #[schemars(description = "The tag name, referenced as @tag_name in task descriptions")]
    pub tag_name: String,
    #[schemars(description = "The content the tag expands to")]
    pub content: String,
    #[schemars(description = "When the tag was last updated")]
    pub updated_at: String,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ListTagsResponse {
    pub tags: Vec<TagSummary>,
    pub count: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateProjectRepoInput {
    #[schemars(description = "Absolute path to the git repository on this machine")]
    pub git_repo_path: String,
    #[schemars(description = "Optional display name. Omit to use the repo folder name.")]
    pub display_name: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CreateProjectRequest {
    #[schemars(description = "The name of the project")]
    pub name: String,
    #[schemars(
        description = "Optional description of what this project is for — helps agents decide where to create tasks"
    )]
    pub description: Option<String>,
    #[schemars(
        description = "Repositories to link to the project. May be empty; repos can be added later in the UI."
    )]
    pub repositories: Vec<CreateProjectRepoInput>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CreateProjectResponse {
    pub project: ProjectSummary,
}

#[derive(Debug, Clone)]
pub struct TaskServer {
    client: reqwest::Client,
    base_url: String,
    tool_router: ToolRouter<TaskServer>,
    context: Option<McpContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct McpRepoContext {
    #[schemars(description = "The unique identifier of the repository")]
    pub repo_id: Uuid,
    #[schemars(description = "The name of the repository")]
    pub repo_name: String,
    #[schemars(description = "The target branch for this repository in this workspace")]
    pub target_branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct McpContext {
    pub project_id: Uuid,
    pub task_id: Uuid,
    pub task_title: String,
    pub workspace_id: Uuid,
    pub workspace_branch: String,
    #[schemars(
        description = "Repository info and target branches for each repo in this workspace"
    )]
    pub workspace_repos: Vec<McpRepoContext>,
}

impl TaskServer {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            tool_router: Self::tool_router(),
            context: None,
        }
    }

    pub async fn init(mut self) -> Self {
        let context = self.fetch_context_at_startup().await;

        if context.is_none() {
            self.tool_router.map.remove("get_context");
            tracing::debug!("VK context not available, get_context tool will not be registered");
        } else {
            tracing::info!("VK context loaded, get_context tool available");
        }

        self.context = context;
        self
    }

    async fn fetch_context_at_startup(&self) -> Option<McpContext> {
        let current_dir = std::env::current_dir().ok()?;
        let canonical_path = current_dir.canonicalize().unwrap_or(current_dir);
        let normalized_path = utils::path::normalize_macos_private_alias(&canonical_path);

        let url = self.url("/api/containers/attempt-context");
        let query = ContainerQuery {
            container_ref: normalized_path.to_string_lossy().to_string(),
        };

        let response = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            self.client.get(&url).query(&query).send(),
        )
        .await
        .ok()?
        .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let api_response: ApiResponseEnvelope<WorkspaceContext> = response.json().await.ok()?;

        if !api_response.success {
            return None;
        }

        let ctx = api_response.data?;

        // Map RepoWithTargetBranch to McpRepoContext
        let workspace_repos: Vec<McpRepoContext> = ctx
            .workspace_repos
            .into_iter()
            .map(|rwb| McpRepoContext {
                repo_id: rwb.repo.id,
                repo_name: rwb.repo.name,
                target_branch: rwb.target_branch,
            })
            .collect();

        Some(McpContext {
            project_id: ctx.project.id,
            task_id: ctx.task.id,
            task_title: ctx.task.title,
            workspace_id: ctx.workspace.id,
            workspace_branch: ctx.workspace.branch,
            workspace_repos,
        })
    }
}

#[derive(Debug, Deserialize)]
struct ApiResponseEnvelope<T> {
    success: bool,
    data: Option<T>,
    message: Option<String>,
}

/// Request body for POST /api/projects (db's CreateProject is Deserialize-only).
#[derive(Debug, Serialize)]
struct CreateProjectBody {
    name: String,
    description: Option<String>,
    repositories: Vec<CreateProjectRepoBody>,
}

#[derive(Debug, Serialize)]
struct CreateProjectRepoBody {
    display_name: String,
    git_repo_path: String,
}

/// Request body for POST /api/sessions/{id}/follow-up (the route's
/// CreateFollowUpAttempt is Deserialize-only).
#[derive(Debug, Serialize)]
struct FollowUpBody {
    prompt: String,
    executor_profile_id: ExecutorProfileId,
    retry_process_id: Option<Uuid>,
    force_when_dirty: Option<bool>,
    perform_git_reset: Option<bool>,
}

/// Request body for POST /api/sessions/{id}/queue (the route's
/// QueueMessageRequest is Deserialize-only).
#[derive(Debug, Serialize)]
struct QueueMessageBody {
    message: String,
    executor_profile_id: ExecutorProfileId,
}

/// Minimal mirror of ExecutionProcess for follow-up responses.
#[derive(Debug, Deserialize)]
struct ExecutionProcessInfo {
    id: Uuid,
    session_id: Uuid,
    status: String,
}

/// Minimal mirror of Session for best-effort executor lookup.
#[derive(Debug, Deserialize)]
struct SessionExecutorInfo {
    executor: Option<String>,
}

/// Minimal mirror of GitBranch (Serialize-only in the git crate).
#[derive(Debug, Deserialize)]
struct BranchInfo {
    name: String,
    is_current: bool,
    is_remote: bool,
}

/// Mirror of WorkspaceSummary (Serialize-only in routes).
#[derive(Debug, Deserialize)]
struct WorkspaceSummaryInfo {
    workspace_id: Uuid,
    latest_session_id: Option<Uuid>,
    has_pending_approval: bool,
    files_changed: Option<usize>,
    lines_added: Option<usize>,
    lines_removed: Option<usize>,
    latest_process_completed_at: Option<String>,
    latest_process_status: Option<String>,
    has_running_dev_server: bool,
    has_unseen_turns: bool,
    pr_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceSummariesEnvelope {
    summaries: Vec<WorkspaceSummaryInfo>,
}

impl TaskServer {
    fn success<T: Serialize>(data: &T) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(data)
                .unwrap_or_else(|_| "Failed to serialize response".to_string()),
        )]))
    }

    fn err_value(v: serde_json::Value) -> Result<CallToolResult, ErrorData> {
        Ok(CallToolResult::error(vec![Content::text(
            serde_json::to_string_pretty(&v)
                .unwrap_or_else(|_| "Failed to serialize error".to_string()),
        )]))
    }

    fn err<S: Into<String>>(msg: S, details: Option<S>) -> Result<CallToolResult, ErrorData> {
        let mut v = serde_json::json!({"success": false, "error": msg.into()});
        if let Some(d) = details {
            v["details"] = serde_json::json!(d.into());
        };
        Self::err_value(v)
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        rb: reqwest::RequestBuilder,
    ) -> Result<T, CallToolResult> {
        let resp = rb
            .send()
            .await
            .map_err(|e| Self::err("Failed to connect to VK API", Some(&e.to_string())).unwrap())?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(
                Self::err(format!("VK API returned error status: {}", status), None).unwrap(),
            );
        }

        let api_response = resp.json::<ApiResponseEnvelope<T>>().await.map_err(|e| {
            Self::err("Failed to parse VK API response", Some(&e.to_string())).unwrap()
        })?;

        if !api_response.success {
            let msg = api_response.message.as_deref().unwrap_or("Unknown error");
            return Err(Self::err("VK API returned error", Some(msg)).unwrap());
        }

        api_response
            .data
            .ok_or_else(|| Self::err("VK API response missing data field", None).unwrap())
    }

    async fn send_empty_json(&self, rb: reqwest::RequestBuilder) -> Result<(), CallToolResult> {
        let resp = rb
            .send()
            .await
            .map_err(|e| Self::err("Failed to connect to VK API", Some(&e.to_string())).unwrap())?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(
                Self::err(format!("VK API returned error status: {}", status), None).unwrap(),
            );
        }

        #[derive(Deserialize)]
        struct EmptyApiResponse {
            success: bool,
            message: Option<String>,
        }

        let api_response = resp.json::<EmptyApiResponse>().await.map_err(|e| {
            Self::err("Failed to parse VK API response", Some(&e.to_string())).unwrap()
        })?;

        if !api_response.success {
            let msg = api_response.message.as_deref().unwrap_or("Unknown error");
            return Err(Self::err("VK API returned error", Some(msg)).unwrap());
        }

        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn resolve_default_executor_profile(
        &self,
        executor: Option<String>,
        variant: Option<String>,
    ) -> Result<(BaseCodingAgent, Option<String>), CallToolResult> {
        let mut executor_str = executor
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let mut variant = variant.and_then(|v| {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        if executor_str.is_none() {
            let info: serde_json::Value = self
                .send_json(self.client.get(self.url("/api/info")))
                .await?;
            executor_str = info
                .pointer("/config/executor_profile/executor")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if variant.is_none() {
                variant = info
                    .pointer("/config/executor_profile/variant")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .filter(|s| !s.is_empty());
            }
        }

        let Some(executor_str) = executor_str else {
            return Err(Self::err(
                "No executor provided and could not read default from /api/info (config.executor_profile)".to_string(),
                None::<String>,
            )
            .unwrap());
        };

        let normalized = executor_str.replace('-', "_").to_ascii_uppercase();
        let base = BaseCodingAgent::from_str(&normalized).map_err(|_| {
            Self::err(
                format!("Unknown executor '{executor_str}'."),
                None::<String>,
            )
            .unwrap()
        })?;
        Ok((base, variant))
    }

    async fn resolve_repo_target_branch(
        &self,
        repo_id: Uuid,
        base_branch: Option<String>,
    ) -> Result<String, CallToolResult> {
        if let Some(branch) = base_branch
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Ok(branch);
        }

        let repo: Repo = self
            .send_json(self.client.get(self.url(&format!("/api/repos/{repo_id}"))))
            .await?;
        if let Some(branch) = repo
            .default_target_branch
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            return Ok(branch);
        }

        Err(Self::err(
            format!(
                "Repository {repo_id} has no default_target_branch. Pass repos[].base_branch explicitly \
(e.g. \"hly-dev\"), or set the repo's default target branch in Helios Kanban. \
Silent fallback to \"main\" was removed because repos without main leave workspaces stuck loading."
            ),
            None::<String>,
        )
        .unwrap())
    }

    async fn project_repo_names(&self, project_id: Uuid) -> Result<Vec<String>, CallToolResult> {
        let url = self.url(&format!("/api/projects/{}/repositories", project_id));
        let repos: Vec<Repo> = self.send_json(self.client.get(&url)).await?;
        Ok(repos
            .into_iter()
            .map(|r| {
                if r.display_name.trim().is_empty() {
                    r.name
                } else {
                    r.display_name
                }
            })
            .collect())
    }

    async fn project_summary(&self, project: Project) -> Result<ProjectSummary, CallToolResult> {
        let repos = self.project_repo_names(project.id).await?;
        Ok(ProjectSummary::from_project_with_repos(project, repos))
    }

    /// Expands @tagname references in text by replacing them with tag content.
    /// Returns the original text if expansion fails (e.g., network error).
    /// Unknown tags are left as-is (not expanded, not an error).
    async fn expand_tags(&self, text: &str) -> String {
        // Pattern matches @tagname where tagname is non-whitespace, non-@ characters
        let tag_pattern = match Regex::new(r"@([^\s@]+)") {
            Ok(re) => re,
            Err(_) => return text.to_string(),
        };

        // Find all unique tag names referenced in the text
        let tag_names: Vec<String> = tag_pattern
            .captures_iter(text)
            .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if tag_names.is_empty() {
            return text.to_string();
        }

        // Fetch all tags from the API
        let url = self.url("/api/tags");
        let tags: Vec<Tag> = match self.client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<ApiResponseEnvelope<Vec<Tag>>>().await {
                    Ok(envelope) if envelope.success => envelope.data.unwrap_or_default(),
                    _ => return text.to_string(),
                }
            }
            _ => return text.to_string(),
        };

        // Build a map of tag_name -> content for quick lookup
        let tag_map: std::collections::HashMap<&str, &str> = tags
            .iter()
            .map(|t| (t.tag_name.as_str(), t.content.as_str()))
            .collect();

        // Replace each @tagname with its content (if found)
        let result = tag_pattern.replace_all(text, |caps: &regex::Captures| {
            let tag_name = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            match tag_map.get(tag_name) {
                Some(content) => (*content).to_string(),
                None => caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string(),
            }
        });

        result.into_owned()
    }

    /// Best-effort lookup of the executor a session has been running with.
    async fn session_executor(&self, session_id: Uuid) -> Option<String> {
        let url = self.url(&format!("/api/sessions/{session_id}"));
        let info: SessionExecutorInfo = self.send_json(self.client.get(&url)).await.ok()?;
        info.executor
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Resolve the executor for session-bound calls (follow-up/queue):
    /// explicit param > the session's current executor > Settings default.
    async fn resolve_session_executor_profile(
        &self,
        session_id: Uuid,
        executor: Option<String>,
        variant: Option<String>,
    ) -> Result<(BaseCodingAgent, Option<String>), CallToolResult> {
        let executor = match executor
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            Some(e) => Some(e),
            None => self.session_executor(session_id).await,
        };
        self.resolve_default_executor_profile(executor, variant)
            .await
    }
}

#[tool_router]
impl TaskServer {
    #[tool(
        description = "Return project, task, and workspace metadata for the current workspace session context."
    )]
    async fn get_context(&self) -> Result<CallToolResult, ErrorData> {
        // Context was fetched at startup and cached
        // This tool is only registered if context exists, so unwrap is safe
        let context = self.context.as_ref().expect("VK context should exist");
        TaskServer::success(context)
    }

    #[tool(
        description = "Create a new task/ticket in a project. Always pass the `project_id` of the project you want to create the task in - it is required! Optionally pass `iteration` (e.g. '260717') to assign the task to an iteration."
    )]
    async fn create_task(
        &self,
        Parameters(CreateTaskRequest {
            project_id,
            title,
            description,
            iteration,
        }): Parameters<CreateTaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        // Expand @tagname references in description
        let expanded_description = match description {
            Some(desc) => Some(self.expand_tags(&desc).await),
            None => None,
        };

        let iteration = iteration.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        let url = self.url("/api/tasks");

        let mut create =
            CreateTask::from_title_description(project_id, title, expanded_description);
        create.iteration = iteration;

        let task: Task = match self.send_json(self.client.post(&url).json(&create)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        TaskServer::success(&CreateTaskResponse {
            task_id: task.id.to_string(),
        })
    }

    #[tool(
        description = "List all available projects with description and linked repo names. Read descriptions to decide which project_id to create tasks in."
    )]
    async fn list_projects(&self) -> Result<CallToolResult, ErrorData> {
        let url = self.url("/api/projects");
        let projects: Vec<Project> = match self.send_json(self.client.get(&url)).await {
            Ok(ps) => ps,
            Err(e) => return Ok(e),
        };

        let mut project_summaries = Vec::with_capacity(projects.len());
        for project in projects {
            match self.project_summary(project).await {
                Ok(summary) => project_summaries.push(summary),
                Err(e) => return Ok(e),
            }
        }

        let response = ListProjectsResponse {
            count: project_summaries.len(),
            projects: project_summaries,
        };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Get a project by id, including description and linked repository names."
    )]
    async fn get_project(
        &self,
        Parameters(GetProjectRequest { project_id }): Parameters<GetProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/projects/{}", project_id));
        let project: Project = match self.send_json(self.client.get(&url)).await {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let summary = match self.project_summary(project).await {
            Ok(s) => s,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&GetProjectResponse { project: summary })
    }

    #[tool(
        description = "Update a project's name and/or description. Use description so other agents know when to create tasks in this project."
    )]
    async fn update_project(
        &self,
        Parameters(UpdateProjectRequest {
            project_id,
            name,
            description,
        }): Parameters<UpdateProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let payload = db::models::project::UpdateProject { name, description };
        let url = self.url(&format!("/api/projects/{}", project_id));
        let project: Project = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };
        let summary = match self.project_summary(project).await {
            Ok(s) => s,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&UpdateProjectResponse { project: summary })
    }

    #[tool(description = "List all repositories for a project. `project_id` is required!")]
    async fn list_repos(
        &self,
        Parameters(ListReposRequest { project_id }): Parameters<ListReposRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/projects/{}/repositories", project_id));
        let repos: Vec<Repo> = match self.send_json(self.client.get(&url)).await {
            Ok(rs) => rs,
            Err(e) => return Ok(e),
        };

        let repo_summaries: Vec<McpRepoSummary> = repos
            .into_iter()
            .map(|r| McpRepoSummary {
                id: r.id.to_string(),
                name: r.name,
            })
            .collect();

        let response = ListReposResponse {
            count: repo_summaries.len(),
            repos: repo_summaries,
            project_id: project_id.to_string(),
        };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Get detailed information about a repository including its scripts. Use `list_repos` to find available repo IDs."
    )]
    async fn get_repo(
        &self,
        Parameters(GetRepoRequest { repo_id }): Parameters<GetRepoRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/repos/{}", repo_id));
        let repo: Repo = match self.send_json(self.client.get(&url)).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&RepoDetails {
            id: repo.id.to_string(),
            name: repo.name,
            display_name: repo.display_name,
            setup_script: repo.setup_script,
            cleanup_script: repo.cleanup_script,
            dev_server_script: repo.dev_server_script,
        })
    }

    #[tool(
        description = "Update a repository's setup script. The setup script runs when initializing a workspace."
    )]
    async fn update_setup_script(
        &self,
        Parameters(UpdateSetupScriptRequest { repo_id, script }): Parameters<
            UpdateSetupScriptRequest,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/repos/{}", repo_id));
        let script_value = if script.is_empty() {
            None
        } else {
            Some(script)
        };
        let payload = serde_json::json!({
            "setup_script": script_value
        });
        let _repo: Repo = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&UpdateRepoScriptResponse {
            success: true,
            repo_id: repo_id.to_string(),
            field: "setup_script".to_string(),
        })
    }

    #[tool(
        description = "Update a repository's cleanup script. The cleanup script runs when tearing down a workspace."
    )]
    async fn update_cleanup_script(
        &self,
        Parameters(UpdateCleanupScriptRequest { repo_id, script }): Parameters<
            UpdateCleanupScriptRequest,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/repos/{}", repo_id));
        let script_value = if script.is_empty() {
            None
        } else {
            Some(script)
        };
        let payload = serde_json::json!({
            "cleanup_script": script_value
        });
        let _repo: Repo = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&UpdateRepoScriptResponse {
            success: true,
            repo_id: repo_id.to_string(),
            field: "cleanup_script".to_string(),
        })
    }

    #[tool(
        description = "Update a repository's dev server script. The dev server script starts the development server for the repository."
    )]
    async fn update_dev_server_script(
        &self,
        Parameters(UpdateDevServerScriptRequest { repo_id, script }): Parameters<
            UpdateDevServerScriptRequest,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/repos/{}", repo_id));
        let script_value = if script.is_empty() {
            None
        } else {
            Some(script)
        };
        let payload = serde_json::json!({
            "dev_server_script": script_value
        });
        let _repo: Repo = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(r) => r,
            Err(e) => return Ok(e),
        };
        TaskServer::success(&UpdateRepoScriptResponse {
            success: true,
            repo_id: repo_id.to_string(),
            field: "dev_server_script".to_string(),
        })
    }

    #[tool(
        description = "List all the task/tickets in a project with optional filtering and execution status. `project_id` is required!"
    )]
    async fn list_tasks(
        &self,
        Parameters(ListTasksRequest {
            project_id,
            status,
            iteration,
            query,
            limit,
        }): Parameters<ListTasksRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let status_filter = if let Some(ref status_str) = status {
            match TaskStatus::from_str(status_str) {
                Ok(s) => Some(s),
                Err(_) => {
                    return Self::err(
                        "Invalid status filter. Valid values: 'todo', 'inprogress', 'inreview', 'done', 'cancelled'".to_string(),
                        Some(status_str.to_string()),
                    );
                }
            }
        } else {
            None
        };

        let url = self.url(&format!("/api/tasks?project_id={}", project_id));
        let all_tasks: Vec<TaskWithAttemptStatus> =
            match self.send_json(self.client.get(&url)).await {
                Ok(t) => t,
                Err(e) => return Ok(e),
            };

        let iteration_filter = iteration
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let query_filter = query
            .as_ref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty());

        let task_limit = limit.unwrap_or(50).max(0) as usize;
        let filtered = all_tasks.into_iter().filter(|t| {
            if let Some(ref want) = status_filter
                && &t.status != want
            {
                return false;
            }
            if let Some(ref want_it) = iteration_filter
                && t.iteration.as_deref() != Some(want_it.as_str())
            {
                return false;
            }
            if let Some(ref q) = query_filter {
                let hay = format!(
                    "{} {}",
                    t.title,
                    t.description.as_deref().unwrap_or("")
                )
                .to_ascii_lowercase();
                if !hay.contains(q) {
                    return false;
                }
            }
            true
        });
        let limited: Vec<TaskWithAttemptStatus> = filtered.take(task_limit).collect();

        let task_summaries: Vec<TaskSummary> = limited
            .into_iter()
            .map(TaskSummary::from_task_with_status)
            .collect();

        let response = ListTasksResponse {
            count: task_summaries.len(),
            tasks: task_summaries,
            project_id: project_id.to_string(),
            applied_filters: ListTasksFilters {
                status: status.clone(),
                iteration: iteration_filter,
                query: query.clone(),
                limit: task_limit as i32,
            },
        };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Start working on a task by creating and launching a new workspace session. Omit `executor` to use Settings default. Omit each repo `base_branch` to use that repo's default_target_branch."
    )]
    async fn start_workspace_session(
        &self,
        Parameters(StartWorkspaceSessionRequest {
            task_id,
            executor,
            variant,
            repos,
        }): Parameters<StartWorkspaceSessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if repos.is_empty() {
            return Self::err(
                "At least one repository must be specified.".to_string(),
                None::<String>,
            );
        }

        let (base_executor, variant) = match self
            .resolve_default_executor_profile(executor, variant)
            .await
        {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };

        let executor_profile_id = ExecutorProfileId {
            executor: base_executor,
            variant,
        };

        let mut workspace_repos: Vec<WorkspaceRepoInput> = Vec::with_capacity(repos.len());
        for r in repos {
            let target_branch = match self
                .resolve_repo_target_branch(r.repo_id, r.base_branch)
                .await
            {
                Ok(b) => b,
                Err(e) => return Ok(e),
            };
            workspace_repos.push(WorkspaceRepoInput {
                repo_id: r.repo_id,
                target_branch,
            });
        }

        let payload = CreateTaskAttemptBody {
            task_id,
            executor_profile_id,
            repos: workspace_repos,
        };

        let url = self.url("/api/task-attempts");
        let workspace: Workspace = match self.send_json(self.client.post(&url).json(&payload)).await
        {
            Ok(workspace) => workspace,
            Err(e) => return Ok(e),
        };

        let response = StartWorkspaceSessionResponse {
            task_id: workspace.task_id.to_string(),
            workspace_id: workspace.id.to_string(),
        };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Stop a running workspace/agent session. Does not cancel or delete the task."
    )]
    async fn stop_workspace_session(
        &self,
        Parameters(StopWorkspaceSessionRequest { workspace_id }): Parameters<
            StopWorkspaceSessionRequest,
        >,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/task-attempts/{}/stop", workspace_id));
        if let Err(e) = self
            .send_empty_json(self.client.post(&url).json(&serde_json::json!({})))
            .await
        {
            return Ok(e);
        }
        TaskServer::success(&StopWorkspaceSessionResponse {
            workspace_id: workspace_id.to_string(),
            stopped: true,
        })
    }

    #[tool(
        description = "Soft-cancel a task (sets status to 'cancelled'). Prefer this over delete_task unless permanent removal is required."
    )]
    async fn cancel_task(
        &self,
        Parameters(CancelTaskRequest { task_id }): Parameters<CancelTaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        // Best-effort stop of workspaces for this task
        let attempts_url = self.url(&format!("/api/task-attempts?task_id={}", task_id));
        if let Ok(workspaces) = self
            .send_json::<Vec<Workspace>>(self.client.get(&attempts_url))
            .await
        {
            for ws in workspaces {
                let stop_url = self.url(&format!("/api/task-attempts/{}/stop", ws.id));
                let _ = self
                    .send_empty_json(self.client.post(&stop_url).json(&serde_json::json!({})))
                    .await;
            }
        }

        let payload = UpdateTask {
            title: None,
            description: None,
            status: Some(TaskStatus::Cancelled),
            parent_workspace_id: None,
            image_ids: None,
            iteration: None,
        };
        let url = self.url(&format!("/api/tasks/{}", task_id));
        let updated_task: Task = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        TaskServer::success(&CancelTaskResponse {
            task: TaskDetails::from_task(updated_task),
        })
    }

    #[tool(
        description = "Update an existing task/ticket's title, description, status, or iteration. `task_id` is required. `title`, `description`, `status`, and `iteration` are optional."
    )]
    async fn update_task(
        &self,
        Parameters(UpdateTaskRequest {
            task_id,
            title,
            description,
            status,
            iteration,
        }): Parameters<UpdateTaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let status = if let Some(ref status_str) = status {
            match TaskStatus::from_str(status_str) {
                Ok(s) => Some(s),
                Err(_) => {
                    return Self::err(
                        "Invalid status filter. Valid values: 'todo', 'inprogress', 'inreview', 'done', 'cancelled'".to_string(),
                        Some(status_str.to_string()),
                    );
                }
            }
        } else {
            None
        };

        // Expand @tagname references in description
        let expanded_description = match description {
            Some(desc) => Some(self.expand_tags(&desc).await),
            None => None,
        };

        let payload = UpdateTask {
            title,
            description: expanded_description,
            status,
            parent_workspace_id: None,
            image_ids: None,
            iteration,
        };
        let url = self.url(&format!("/api/tasks/{}", task_id));
        let updated_task: Task = match self.send_json(self.client.put(&url).json(&payload)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let details = TaskDetails::from_task(updated_task);
        let response = UpdateTaskResponse { task: details };
        TaskServer::success(&response)
    }

    #[tool(description = "Delete a task/ticket. `task_id` is required.")]
    async fn delete_task(
        &self,
        Parameters(DeleteTaskRequest { task_id }): Parameters<DeleteTaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/tasks/{}", task_id));
        if let Err(e) = self.send_empty_json(self.client.delete(&url)).await {
            return Ok(e);
        }

        let response = DeleteTaskResponse {
            deleted_task_id: Some(task_id.to_string()),
        };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Get detailed information (like task description) about a specific task/ticket. You can use `list_tasks` to find the `task_ids` of all tasks in a project. `task_id` is required."
    )]
    async fn get_task(
        &self,
        Parameters(GetTaskRequest { task_id }): Parameters<GetTaskRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/tasks/{}", task_id));
        let task: Task = match self.send_json(self.client.get(&url)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let details = TaskDetails::from_task(task);
        let response = GetTaskResponse { task: details };

        TaskServer::success(&response)
    }

    #[tool(
        description = "Send a follow-up instruction to a task's agent session (starts a new execution in the same workspace). Use get_task_status to find a workspace's latest_session_id. Omit `executor` to reuse the session's executor."
    )]
    async fn follow_up_session(
        &self,
        Parameters(FollowUpSessionRequest {
            session_id,
            prompt,
            executor,
            variant,
        }): Parameters<FollowUpSessionRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (base_executor, variant) = match self
            .resolve_session_executor_profile(session_id, executor, variant)
            .await
        {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };

        let payload = FollowUpBody {
            prompt,
            executor_profile_id: ExecutorProfileId {
                executor: base_executor,
                variant,
            },
            retry_process_id: None,
            force_when_dirty: None,
            perform_git_reset: None,
        };

        let url = self.url(&format!("/api/sessions/{session_id}/follow-up"));
        let process: ExecutionProcessInfo =
            match self.send_json(self.client.post(&url).json(&payload)).await {
                Ok(p) => p,
                Err(e) => return Ok(e),
            };

        TaskServer::success(&FollowUpSessionResponse {
            session_id: process.session_id.to_string(),
            execution_process_id: process.id.to_string(),
            status: process.status,
        })
    }

    #[tool(
        description = "Queue a message to be delivered to a session when its current execution finishes (one queued message per session; a new one replaces the old). Prefer this over follow_up_session when the agent is still running. Use get_task_status to find a workspace's latest_session_id."
    )]
    async fn queue_message(
        &self,
        Parameters(QueueMessageRequest {
            session_id,
            message,
            executor,
            variant,
        }): Parameters<QueueMessageRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let (base_executor, variant) = match self
            .resolve_session_executor_profile(session_id, executor, variant)
            .await
        {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };

        let payload = QueueMessageBody {
            message,
            executor_profile_id: ExecutorProfileId {
                executor: base_executor,
                variant,
            },
        };

        let url = self.url(&format!("/api/sessions/{session_id}/queue"));
        let status: serde_json::Value =
            match self.send_json(self.client.post(&url).json(&payload)).await {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

        let queued = status.get("status").and_then(|s| s.as_str()) == Some("queued");

        TaskServer::success(&QueueMessageResponse {
            session_id: session_id.to_string(),
            queued,
        })
    }

    #[tool(
        description = "List pending tool-approval requests from running agents. Respond to one with respond_to_approval."
    )]
    async fn list_approvals(&self) -> Result<CallToolResult, ErrorData> {
        let url = self.url("/api/approvals");
        let approvals: Vec<ApprovalInfo> = match self.send_json(self.client.get(&url)).await {
            Ok(a) => a,
            Err(e) => return Ok(e),
        };

        let summaries: Vec<ApprovalSummary> = approvals
            .into_iter()
            .map(|a| ApprovalSummary {
                approval_id: a.approval_id,
                tool_name: a.tool_name,
                execution_process_id: a.execution_process_id.to_string(),
                is_question: a.is_question,
                created_at: a.created_at.to_rfc3339(),
                timeout_at: a.timeout_at.to_rfc3339(),
            })
            .collect();

        TaskServer::success(&ListApprovalsResponse {
            count: summaries.len(),
            approvals: summaries,
        })
    }

    #[tool(
        description = "Approve or deny a pending tool-approval request. Get the `approval_id` from list_approvals. `decision` must be 'approved' or 'denied'."
    )]
    async fn respond_to_approval(
        &self,
        Parameters(RespondToApprovalRequest {
            approval_id,
            decision,
            reason,
        }): Parameters<RespondToApprovalRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let status = match decision.trim().to_ascii_lowercase().as_str() {
            "approved" => ApprovalOutcome::Approved,
            "denied" => ApprovalOutcome::Denied {
                reason: reason
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty()),
            },
            _ => {
                return Self::err(
                    "Invalid decision. Valid values: 'approved', 'denied'".to_string(),
                    Some(decision),
                );
            }
        };

        // The respond endpoint requires the execution_process_id; look it up
        // from the pending approvals list so callers only need the approval_id.
        let url = self.url("/api/approvals");
        let approvals: Vec<ApprovalInfo> = match self.send_json(self.client.get(&url)).await {
            Ok(a) => a,
            Err(e) => return Ok(e),
        };
        let Some(approval) = approvals.into_iter().find(|a| a.approval_id == approval_id) else {
            return Self::err(
                format!(
                    "Approval {approval_id} not found among pending approvals (already resolved or timed out?)."
                ),
                None::<String>,
            );
        };
        if approval.is_question {
            return Self::err(
                "This request is a question from the agent, not an approve/deny decision; it cannot be answered with this tool."
                    .to_string(),
                None::<String>,
            );
        }

        let payload = ApprovalResponse {
            execution_process_id: approval.execution_process_id,
            status,
        };
        let url = self.url(&format!("/api/approvals/{approval_id}/respond"));
        let outcome: ApprovalOutcome =
            match self.send_json(self.client.post(&url).json(&payload)).await {
                Ok(o) => o,
                Err(e) => return Ok(e),
            };

        let outcome_label = match &outcome {
            ApprovalOutcome::Approved => "approved",
            ApprovalOutcome::Denied { .. } => "denied",
            ApprovalOutcome::Answered { .. } => "answered",
            ApprovalOutcome::TimedOut => "timed_out",
        };

        TaskServer::success(&RespondToApprovalResponse {
            approval_id,
            outcome: outcome_label.to_string(),
        })
    }

    #[tool(
        description = "Create a new task in a project and immediately start a workspace session on it. Equivalent to create_task + start_workspace_session in one call."
    )]
    async fn create_task_and_start(
        &self,
        Parameters(CreateTaskAndStartRequest {
            project_id,
            title,
            description,
            iteration,
            executor,
            variant,
            repos,
        }): Parameters<CreateTaskAndStartRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        if repos.is_empty() {
            return Self::err(
                "At least one repository must be specified.".to_string(),
                None::<String>,
            );
        }

        // 1. Create the task (same behavior as create_task)
        let expanded_description = match description {
            Some(desc) => Some(self.expand_tags(&desc).await),
            None => None,
        };
        let iteration = iteration.and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        let mut create =
            CreateTask::from_title_description(project_id, title, expanded_description);
        create.iteration = iteration;

        let url = self.url("/api/tasks");
        let task: Task = match self.send_json(self.client.post(&url).json(&create)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        // 2. Start a workspace session on the new task (same behavior as start_workspace_session)
        let (base_executor, variant) = match self
            .resolve_default_executor_profile(executor, variant)
            .await
        {
            Ok(v) => v,
            Err(e) => return Ok(e),
        };
        let executor_profile_id = ExecutorProfileId {
            executor: base_executor,
            variant,
        };

        let mut workspace_repos: Vec<WorkspaceRepoInput> = Vec::with_capacity(repos.len());
        for r in repos {
            let target_branch = match self
                .resolve_repo_target_branch(r.repo_id, r.base_branch)
                .await
            {
                Ok(b) => b,
                Err(e) => return Ok(e),
            };
            workspace_repos.push(WorkspaceRepoInput {
                repo_id: r.repo_id,
                target_branch,
            });
        }

        let payload = CreateTaskAttemptBody {
            task_id: task.id,
            executor_profile_id,
            repos: workspace_repos,
        };
        let url = self.url("/api/task-attempts");
        let workspace: Workspace = match self.send_json(self.client.post(&url).json(&payload)).await
        {
            Ok(w) => w,
            Err(_) => {
                return Self::err(
                    format!(
                        "Task {} was created but starting the workspace session failed. Do NOT create the task again; call start_workspace_session with this task_id to see the underlying error.",
                        task.id
                    ),
                    None::<String>,
                );
            }
        };

        TaskServer::success(&CreateTaskAndStartResponse {
            task_id: task.id.to_string(),
            workspace_id: workspace.id.to_string(),
        })
    }

    #[tool(
        description = "Get the aggregated status of a task: task details plus, for each of its workspaces, the latest execution status, session ID (for follow_up_session/queue_message), pending-approval flag, PR status, and diff stats (files changed, lines added/removed)."
    )]
    async fn get_task_status(
        &self,
        Parameters(GetTaskStatusRequest { task_id }): Parameters<GetTaskStatusRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/tasks/{task_id}"));
        let task: Task = match self.send_json(self.client.get(&url)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let url = self.url(&format!("/api/task-attempts?task_id={task_id}"));
        let mut workspaces: Vec<Workspace> = match self.send_json(self.client.get(&url)).await {
            Ok(ws) => ws,
            Err(e) => return Ok(e),
        };
        workspaces.sort_by_key(|w| w.created_at);

        // Workspace summaries are only available per archived-flag, so fetch
        // both variants and index by workspace_id.
        let mut summaries: std::collections::HashMap<Uuid, WorkspaceSummaryInfo> =
            std::collections::HashMap::new();
        if !workspaces.is_empty() {
            for archived in [false, true] {
                if archived && !workspaces.iter().any(|w| w.archived) {
                    continue;
                }
                let query = WorkspaceSummaryRequest { archived };
                let url = self.url("/api/task-attempts/summary");
                let envelope: WorkspaceSummariesEnvelope =
                    match self.send_json(self.client.post(&url).json(&query)).await {
                        Ok(e) => e,
                        Err(e) => return Ok(e),
                    };
                for s in envelope.summaries {
                    summaries.insert(s.workspace_id, s);
                }
            }
        }

        let statuses: Vec<TaskWorkspaceStatus> = workspaces
            .into_iter()
            .map(|ws| {
                let s = summaries.get(&ws.id);
                TaskWorkspaceStatus {
                    workspace_id: ws.id.to_string(),
                    name: ws.name,
                    branch: ws.branch,
                    archived: ws.archived,
                    created_at: ws.created_at.to_rfc3339(),
                    latest_session_id: s.and_then(|x| x.latest_session_id.map(|id| id.to_string())),
                    latest_process_status: s.and_then(|x| x.latest_process_status.clone()),
                    latest_process_completed_at: s
                        .and_then(|x| x.latest_process_completed_at.clone()),
                    has_pending_approval: s.is_some_and(|x| x.has_pending_approval),
                    has_running_dev_server: s.is_some_and(|x| x.has_running_dev_server),
                    has_unseen_turns: s.is_some_and(|x| x.has_unseen_turns),
                    pr_status: s.and_then(|x| x.pr_status.clone()),
                    files_changed: s.and_then(|x| x.files_changed),
                    lines_added: s.and_then(|x| x.lines_added),
                    lines_removed: s.and_then(|x| x.lines_removed),
                }
            })
            .collect();

        TaskServer::success(&GetTaskStatusResponse {
            task: TaskDetails::from_task(task),
            workspaces: statuses,
        })
    }

    #[tool(
        description = "List the git branches (local and remote) of a repository. Use it to pick a valid base_branch for start_workspace_session or create_task_and_start."
    )]
    async fn list_branches(
        &self,
        Parameters(ListBranchesRequest { repo_id }): Parameters<ListBranchesRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let url = self.url(&format!("/api/repos/{repo_id}/branches"));
        let branches: Vec<BranchInfo> = match self.send_json(self.client.get(&url)).await {
            Ok(b) => b,
            Err(e) => return Ok(e),
        };

        let summaries: Vec<BranchSummary> = branches
            .into_iter()
            .map(|b| BranchSummary {
                name: b.name,
                is_current: b.is_current,
                is_remote: b.is_remote,
            })
            .collect();

        TaskServer::success(&ListBranchesResponse {
            repo_id: repo_id.to_string(),
            count: summaries.len(),
            branches: summaries,
        })
    }

    #[tool(
        description = "List all tags. Reference a tag in a task description as @tag_name to expand its content into the description."
    )]
    async fn list_tags(&self) -> Result<CallToolResult, ErrorData> {
        let url = self.url("/api/tags");
        let tags: Vec<Tag> = match self.send_json(self.client.get(&url)).await {
            Ok(t) => t,
            Err(e) => return Ok(e),
        };

        let summaries: Vec<TagSummary> = tags
            .into_iter()
            .map(|t| TagSummary {
                id: t.id.to_string(),
                tag_name: t.tag_name,
                content: t.content,
                updated_at: t.updated_at.to_rfc3339(),
            })
            .collect();

        TaskServer::success(&ListTagsResponse {
            count: summaries.len(),
            tags: summaries,
        })
    }

    #[tool(
        description = "Create a new project. Optionally link repositories by passing their absolute git_repo_path (repos can also be added later in the UI)."
    )]
    async fn create_project(
        &self,
        Parameters(CreateProjectRequest {
            name,
            description,
            repositories,
        }): Parameters<CreateProjectRequest>,
    ) -> Result<CallToolResult, ErrorData> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Self::err(
                "Project name must not be empty.".to_string(),
                None::<String>,
            );
        }

        let mut repos: Vec<CreateProjectRepoBody> = Vec::with_capacity(repositories.len());
        for r in repositories {
            let path = r.git_repo_path.trim().trim_end_matches('/').to_string();
            if path.is_empty() {
                return Self::err(
                    "git_repo_path must not be empty.".to_string(),
                    None::<String>,
                );
            }
            let display_name = r
                .display_name
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| path.clone())
                });
            repos.push(CreateProjectRepoBody {
                display_name,
                git_repo_path: path,
            });
        }

        let description = description
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let payload = CreateProjectBody {
            name,
            description,
            repositories: repos,
        };

        let url = self.url("/api/projects");
        let project: Project = match self.send_json(self.client.post(&url).json(&payload)).await {
            Ok(p) => p,
            Err(e) => return Ok(e),
        };

        let summary = match self.project_summary(project).await {
            Ok(s) => s,
            Err(e) => return Ok(e),
        };

        TaskServer::success(&CreateProjectResponse { project: summary })
    }
}

#[tool_handler]
impl ServerHandler for TaskServer {
    fn get_info(&self) -> ServerInfo {
        let mut instruction = "A task and project management server. Always call `list_projects` first and read each project's `description` (and `repos`) to choose the correct `project_id` before creating tasks. TOOLS: 'list_projects', 'get_project', 'create_project', 'update_project', 'list_tasks', 'create_task', 'create_task_and_start', 'get_task', 'get_task_status', 'update_task', 'cancel_task', 'delete_task', 'start_workspace_session', 'stop_workspace_session', 'follow_up_session', 'queue_message', 'list_approvals', 'respond_to_approval', 'list_repos', 'get_repo', 'list_branches', 'list_tags', 'update_setup_script', 'update_cleanup_script', 'update_dev_server_script'. Omit executor on start_workspace_session to use Settings default; omit base_branch to use repo default_target_branch. Prefer cancel_task over delete_task. Prefer create_task_and_start over separate create_task + start_workspace_session calls.".to_string();
        if self.context.is_some() {
            let context_instruction = "Use 'get_context' to fetch project/task/workspace metadata for the active Vibe Kanban workspace session when available.";
            instruction = format!("{} {}", context_instruction, instruction);
        }

        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_03_26,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "vibe-kanban".to_string(),
                version: "1.0.0".to_string(),
            },
            instructions: Some(instruction),
        }
    }
}
