use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use db::{
    DBService,
    models::{
        execution_process_repo_state::ExecutionProcessRepoState, merge::Merge, repo::Repo,
        workspace::Workspace, workspace_repo::WorkspaceRepo,
    },
};
use executors::logs::utils::{ConversationPatch, patch::escape_json_pointer_segment};
use futures::StreamExt;
use git::{Commit, DiffTarget, GitService, GitServiceError};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
};
use sqlx::SqlitePool;
use thiserror::Error;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_stream::wrappers::{IntervalStream, ReceiverStream};
use utils::{
    diff::{self, Diff},
    log_msg::LogMsg,
};
use uuid::Uuid;

use crate::services::filesystem_watcher::{self, FilesystemWatcherError};

#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub files_changed: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// Resolve the base commit for diffing a live (unmerged) repo.
/// Returns the commit plus whether it is pinned. Repos without worktrees are
/// pinned to the earliest recorded `before_head_commit`, because their branch
/// is the target branch itself and `merge_base` against its live HEAD
/// becomes meaningless once the branch advances.
pub async fn resolve_base_commit(
    pool: &SqlitePool,
    git: &GitService,
    workspace: &Workspace,
    repo: &Repo,
    target_branch: &str,
) -> Option<(Commit, bool)> {
    if !repo.use_worktree
        && let Ok(Some(sha)) =
            ExecutionProcessRepoState::find_earliest_before_head_commit(
                pool,
                workspace.id,
                repo.id,
            )
            .await
        && let Ok(oid) = git2::Oid::from_str(&sha)
    {
        return Some((Commit::new(oid), true));
    }

    let git = git.clone();
    let repo_path = repo.path.clone();
    let branch = workspace.branch.clone();
    let target = target_branch.to_string();
    tokio::task::spawn_blocking(move || git.get_base_commit(&repo_path, &branch, &target))
        .await
        .ok()
        .and_then(Result::ok)
        .map(|commit| (commit, false))
}

/// A merge commit recorded for a workspace repo.
#[derive(Debug, Clone)]
pub struct MergeCommit {
    pub sha: String,
    /// Direct merges rewrite the local task branch to point at the merge
    /// commit, which lets us detect new work on top of the merge. PR merges
    /// leave the local branch untouched, so this detection is not possible.
    pub direct: bool,
}

/// Latest merge commit per repo for a workspace, keyed by repo id.
/// `Merge::find_by_workspace_id` returns rows newest-first, so the first
/// commit seen per repo wins.
pub async fn merge_commits_by_repo(
    pool: &SqlitePool,
    workspace: &Workspace,
) -> Result<HashMap<Uuid, MergeCommit>, sqlx::Error> {
    let merges = Merge::find_by_workspace_id(pool, workspace.id).await?;
    let mut by_repo = HashMap::new();
    for merge in merges {
        let direct = matches!(merge, Merge::Direct(_));
        if let Some(sha) = merge.merge_commit() {
            by_repo
                .entry(merge.repo_id())
                .or_insert(MergeCommit { sha, direct });
        }
    }
    Ok(by_repo)
}

/// Whether new work exists on top of a direct merge: the branch moved past
/// the merge commit, or an existing worktree has uncommitted changes. In
/// both cases a live worktree diff (against the merge base) reflects the
/// current state; otherwise the static merge-commit diff is authoritative.
pub fn has_work_since_merge(
    git: &GitService,
    repo_path: &Path,
    branch: &str,
    merge_commit: &str,
    worktree_path: Option<&Path>,
) -> bool {
    match branch_head_commit(repo_path, branch) {
        // Branch deleted: the merge commit diff is all we can show.
        None => return false,
        Some(head) if head == merge_commit => {}
        Some(_) => return true,
    }

    if let Some(worktree) = worktree_path
        && worktree.exists()
        && matches!(git.is_worktree_clean(worktree), Ok(false))
    {
        return true;
    }

    false
}

fn branch_head_commit(repo_path: &Path, branch: &str) -> Option<String> {
    let repo = git2::Repository::open(repo_path).ok()?;
    let branch = repo.find_branch(branch, git2::BranchType::Local).ok()?;
    Some(branch.get().peel_to_commit().ok()?.id().to_string())
}

/// Computes diff stats for a workspace by comparing against target branches.
pub async fn compute_diff_stats(
    pool: &SqlitePool,
    git: &GitService,
    workspace: &Workspace,
) -> Option<DiffStats> {
    let workspace_repos =
        WorkspaceRepo::find_repos_with_target_branch_for_workspace(pool, workspace.id)
            .await
            .ok()?;

    let merge_commits = merge_commits_by_repo(pool, workspace).await.ok()?;

    // Without a container ref we can only report stats for merged repos.
    if workspace.container_ref.is_none() && merge_commits.is_empty() {
        return None;
    }

    let mut stats = DiffStats::default();

    for repo_with_branch in workspace_repos {
        let repo = repo_with_branch.repo;

        // Merged repos: diff the merge commit against its parent. Works even
        // after the worktree has been cleaned up or the branch deleted. If a
        // direct merge was followed by new work, the live worktree diff below
        // is authoritative instead.
        let merged_diffs = match merge_commits.get(&repo.id) {
            Some(merge_commit) => {
                let restarted = merge_commit.direct && {
                    let git = git.clone();
                    let repo_path = repo.path.clone();
                    let branch = workspace.branch.clone();
                    let sha = merge_commit.sha.clone();
                    let worktree_path = workspace.container_ref.as_ref().map(|container_ref| {
                        if repo.use_worktree {
                            PathBuf::from(container_ref).join(&repo.name)
                        } else {
                            repo.path.clone()
                        }
                    });
                    tokio::task::spawn_blocking(move || {
                        has_work_since_merge(
                            &git,
                            &repo_path,
                            &branch,
                            &sha,
                            worktree_path.as_deref(),
                        )
                    })
                    .await
                    .unwrap_or(false)
                };
                if restarted {
                    None
                } else {
                    let git = git.clone();
                    let repo_path = repo.path.clone();
                    let commit_sha = merge_commit.sha.clone();
                    tokio::task::spawn_blocking(move || {
                        git.get_diffs(
                            DiffTarget::Commit {
                                repo_path: &repo_path,
                                commit_sha: &commit_sha,
                            },
                            None,
                        )
                    })
                    .await
                    .ok()
                    .and_then(Result::ok)
                }
            }
            None => None,
        };

        let diffs = match merged_diffs {
            Some(diffs) => diffs,
            None => {
                let Some(container_ref) = workspace.container_ref.as_ref() else {
                    continue;
                };
                let worktree_path = if repo.use_worktree {
                    PathBuf::from(container_ref).join(&repo.name)
                } else {
                    repo.path.clone()
                };

                let Some((base_commit, _pinned)) = resolve_base_commit(
                    pool,
                    git,
                    workspace,
                    &repo,
                    &repo_with_branch.target_branch,
                )
                .await
                else {
                    continue;
                };

                let git = git.clone();
                let worktree = worktree_path.clone();
                match tokio::task::spawn_blocking(move || {
                    git.get_diffs(
                        DiffTarget::Worktree {
                            worktree_path: &worktree,
                            base_commit: &base_commit,
                        },
                        None,
                    )
                })
                .await
                {
                    Ok(Ok(diffs)) => diffs,
                    _ => continue,
                }
            }
        };

        for diff in diffs {
            stats.files_changed += 1;
            stats.lines_added += diff.additions.unwrap_or(0);
            stats.lines_removed += diff.deletions.unwrap_or(0);
        }
    }

    Some(stats)
}

/// Maximum cumulative diff bytes to stream before omitting content (200MB)
pub const MAX_CUMULATIVE_DIFF_BYTES: usize = 200 * 1024 * 1024;

const DIFF_STREAM_CHANNEL_CAPACITY: usize = 1000;

/// Errors that can occur during diff stream creation and operation
#[derive(Error, Debug)]
pub enum DiffStreamError {
    #[error("Git service error: {0}")]
    GitService(#[from] GitServiceError),
    #[error("Filesystem watcher error: {0}")]
    FilesystemWatcher(#[from] FilesystemWatcherError),
    #[error("Task join error: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Notify error: {0}")]
    Notify(#[from] notify::Error),
}

/// Diff stream that owns the filesystem watcher task
/// When this stream is dropped, the watcher is automatically cleaned up
pub struct DiffStreamHandle {
    stream: futures::stream::BoxStream<'static, Result<LogMsg, io::Error>>,
    _watcher_task: Option<JoinHandle<()>>,
}

impl futures::Stream for DiffStreamHandle {
    type Item = Result<LogMsg, io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // Delegate to inner stream
        std::pin::Pin::new(&mut self.stream).poll_next(cx)
    }
}

impl Drop for DiffStreamHandle {
    fn drop(&mut self) {
        if let Some(handle) = self._watcher_task.take() {
            handle.abort();
        }
    }
}

impl DiffStreamHandle {
    /// Create a new DiffStreamHandle from a boxed stream and optional watcher task
    pub fn new(
        stream: futures::stream::BoxStream<'static, Result<LogMsg, io::Error>>,
        watcher_task: Option<JoinHandle<()>>,
    ) -> Self {
        Self {
            stream,
            _watcher_task: watcher_task,
        }
    }
}

#[derive(Clone)]
pub struct DiffStreamArgs {
    pub git_service: GitService,
    pub db: DBService,
    pub workspace_id: Uuid,
    pub repo_id: Uuid,
    pub repo_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub target_branch: String,
    pub base_commit: Commit,
    /// When true, `base_commit` is pinned (e.g. to a recorded
    /// `before_head_commit`) and must not be recomputed from live refs.
    pub pin_base_commit: bool,
    pub stats_only: bool,
    pub path_prefix: Option<String>,
}

struct DiffStreamManager {
    args: DiffStreamArgs,
    tx: mpsc::Sender<Result<LogMsg, io::Error>>,
    cumulative: Arc<AtomicUsize>,
    known_paths: Arc<std::sync::RwLock<HashSet<String>>>,
    full_sent: Arc<std::sync::RwLock<HashSet<String>>>,
    current_base_commit: Commit,
    current_target_branch: String,
}

enum DiffEvent {
    Filesystem(DebounceEventResult),
    GitStateChange,
    CheckTarget,
}

pub async fn create(args: DiffStreamArgs) -> Result<DiffStreamHandle, DiffStreamError> {
    let (tx, rx) = mpsc::channel::<Result<LogMsg, io::Error>>(DIFF_STREAM_CHANNEL_CAPACITY);
    let manager_args = args.clone();

    let watcher_task = tokio::spawn(async move {
        let mut manager = DiffStreamManager::new(manager_args, tx);
        if let Err(e) = manager.run().await {
            tracing::error!("Diff stream manager failed: {e}");
            let _ = manager.tx.send(Err(io::Error::other(e.to_string()))).await;
        }
    });

    Ok(DiffStreamHandle::new(
        ReceiverStream::new(rx).boxed(),
        Some(watcher_task),
    ))
}

/// Create a static diff stream from a precomputed set of diffs (e.g. the diff
/// of a merge commit for an already-merged workspace). Sends all diffs
/// followed by `Ready`, then stays open without any filesystem watchers.
pub fn create_static(
    diffs: Vec<Diff>,
    repo_id: Uuid,
    path_prefix: Option<String>,
) -> DiffStreamHandle {
    let (tx, rx) = mpsc::channel::<Result<LogMsg, io::Error>>(DIFF_STREAM_CHANNEL_CAPACITY);

    let task = tokio::spawn(async move {
        for diff in diffs {
            let (_raw_path, msg) = decorate_diff(diff, path_prefix.as_deref(), repo_id);
            if tx.send(Ok(msg)).await.is_err() {
                return;
            }
        }
        let _ = tx.send(Ok(LogMsg::Ready)).await;
        // Keep the stream open so the websocket doesn't close, which would
        // trigger reconnect loops; these diffs never change.
        std::future::pending::<()>().await;
    });

    DiffStreamHandle::new(ReceiverStream::new(rx).boxed(), Some(task))
}

impl DiffStreamManager {
    fn new(args: DiffStreamArgs, tx: mpsc::Sender<Result<LogMsg, io::Error>>) -> Self {
        Self {
            current_base_commit: args.base_commit.clone(),
            current_target_branch: args.target_branch.clone(),
            args,
            tx,
            cumulative: Arc::new(AtomicUsize::new(0)),
            known_paths: Arc::new(std::sync::RwLock::new(HashSet::new())),
            full_sent: Arc::new(std::sync::RwLock::new(HashSet::new())),
        }
    }

    async fn run(&mut self) -> Result<(), DiffStreamError> {
        self.reset_stream().await?;

        // Send Ready message to indicate initial data has been sent
        let _ready_error = self.tx.send(Ok(LogMsg::Ready)).await;

        let (fs_debouncer, mut fs_rx, canonical_worktree) =
            filesystem_watcher::async_watcher(self.args.worktree_path.clone())
                .map_err(|e| io::Error::other(e.to_string()))?;
        let _fs_guard = fs_debouncer;

        let (git_debouncer, mut git_rx) =
            match setup_git_watcher(&self.args.git_service, &self.args.worktree_path) {
                Some((d, rx)) => (Some(d), Some(rx)),
                None => (None, None),
            };
        let _git_guard = git_debouncer;

        let mut target_interval =
            IntervalStream::new(tokio::time::interval(Duration::from_secs(1)));

        loop {
            let event = tokio::select! {
                Some(res) = fs_rx.next() => DiffEvent::Filesystem(res),
                Ok(()) = async {
                    match git_rx.as_mut() {
                        Some(rx) => rx.changed().await,
                        None => std::future::pending().await,
                    }
                } => DiffEvent::GitStateChange,
                _ = target_interval.next() => DiffEvent::CheckTarget,
                else => break,
            };

            match event {
                DiffEvent::Filesystem(res) => match res {
                    Ok(events) => {
                        self.handle_fs_events(events, &canonical_worktree).await?;
                    }
                    Err(e) => {
                        tracing::error!("Filesystem watcher error: {e:?}");
                        return Err(io::Error::other(format!("{e:?}")).into());
                    }
                },
                DiffEvent::GitStateChange => {
                    self.handle_git_state_change().await?;
                }
                DiffEvent::CheckTarget => {
                    self.handle_target_check().await?;
                }
            }
        }
        Ok(())
    }

    async fn reset_stream(&mut self) -> Result<(), DiffStreamError> {
        let paths_to_clear: Vec<String> = {
            let mut guard = self.known_paths.write().unwrap();
            guard.drain().collect()
        };

        for raw_path in paths_to_clear {
            let prefixed = prefix_path(raw_path, self.args.path_prefix.as_deref());
            let patch = ConversationPatch::remove_diff(escape_json_pointer_segment(&prefixed));
            if self.tx.send(Ok(LogMsg::JsonPatch(patch))).await.is_err() {
                return Ok(());
            }
        }

        self.cumulative.store(0, Ordering::Relaxed);
        self.full_sent.write().unwrap().clear();

        let diffs = self.fetch_diffs().await?;
        self.send_diffs(diffs).await?;

        Ok(())
    }

    async fn fetch_diffs(&self) -> Result<Vec<Diff>, DiffStreamError> {
        let git = self.args.git_service.clone();
        let worktree = self.args.worktree_path.clone();
        let base = self.current_base_commit.clone();
        let stats_only = self.args.stats_only;
        let cumulative = self.cumulative.clone();

        tokio::task::spawn_blocking(move || {
            let diffs = git.get_diffs(
                DiffTarget::Worktree {
                    worktree_path: &worktree,
                    base_commit: &base,
                },
                None,
            )?;

            let mut processed_diffs = Vec::with_capacity(diffs.len());
            for mut diff in diffs {
                apply_stream_omit_policy(&mut diff, &cumulative, stats_only);
                processed_diffs.push(diff);
            }
            Ok(processed_diffs)
        })
        .await?
    }

    async fn send_diffs(&self, diffs: Vec<Diff>) -> Result<(), DiffStreamError> {
        for diff in diffs {
            let content_omitted = diff.content_omitted;
            let (raw_path, msg) =
                decorate_diff(diff, self.args.path_prefix.as_deref(), self.args.repo_id);

            {
                let mut guard = self.known_paths.write().unwrap();
                guard.insert(raw_path.clone());
            }

            if !content_omitted {
                let mut guard = self.full_sent.write().unwrap();
                guard.insert(raw_path.clone());
            }

            if self.tx.send(Ok(msg)).await.is_err() {
                return Ok(());
            }
        }
        Ok(())
    }

    async fn handle_fs_events(
        &self,
        events: Vec<DebouncedEvent>,
        canonical_worktree: &Path,
    ) -> Result<(), DiffStreamError> {
        let changed_paths =
            extract_changed_paths(&events, canonical_worktree, &self.args.worktree_path);

        if changed_paths.is_empty() {
            return Ok(());
        }

        let git = self.args.git_service.clone();
        let worktree = self.args.worktree_path.clone();
        let base = self.current_base_commit.clone();
        let cumulative = self.cumulative.clone();
        let full_sent = self.full_sent.clone();
        let known_paths = self.known_paths.clone();
        let stats_only = self.args.stats_only;
        let prefix = self.args.path_prefix.clone();
        let repo_id = self.args.repo_id;

        let messages = tokio::task::spawn_blocking(move || {
            process_file_changes(
                &git,
                &worktree,
                &base,
                &changed_paths,
                &cumulative,
                &full_sent,
                &known_paths,
                stats_only,
                prefix.as_deref(),
                repo_id,
            )
        })
        .await??;

        for msg in messages {
            if self.tx.send(Ok(msg)).await.is_err() {
                return Ok(());
            }
        }
        Ok(())
    }

    async fn handle_git_state_change(&mut self) -> Result<(), DiffStreamError> {
        let Some(new_base) = self
            .recompute_base_commit(&self.current_target_branch)
            .await
        else {
            return Ok(());
        };

        if new_base.as_oid() != self.current_base_commit.as_oid() {
            self.current_base_commit = new_base;
            self.reset_stream().await?;
        }
        Ok(())
    }

    async fn handle_target_check(&mut self) -> Result<(), DiffStreamError> {
        let Ok(Some(repo)) = WorkspaceRepo::find_by_workspace_and_repo_id(
            &self.args.db.pool,
            self.args.workspace_id,
            self.args.repo_id,
        )
        .await
        else {
            return Ok(());
        };

        if repo.target_branch != self.current_target_branch
            && let Some(new_base) = self.recompute_base_commit(&repo.target_branch).await
        {
            self.current_target_branch = repo.target_branch;
            self.current_base_commit = new_base;
            self.reset_stream().await?;
        }
        Ok(())
    }

    async fn recompute_base_commit(&self, target_branch: &str) -> Option<Commit> {
        if self.args.pin_base_commit {
            return None;
        }

        let git = self.args.git_service.clone();
        let repo_path = self.args.repo_path.clone();
        let branch = self.args.branch.clone();
        let target = target_branch.to_string();

        tokio::task::spawn_blocking(move || git.get_base_commit(&repo_path, &branch, &target).ok())
            .await
            .ok()
            .flatten()
    }
}

fn prefix_path(path: String, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) => format!("{p}/{path}"),
        None => path,
    }
}

/// Apply path prefixing and repo id to a diff, returning its raw
/// (unprefixed) path and the corresponding `add_diff` patch message.
fn decorate_diff(mut diff: Diff, path_prefix: Option<&str>, repo_id: Uuid) -> (String, LogMsg) {
    let raw_path = GitService::diff_path(&diff);

    let prefixed_entry = prefix_path(raw_path.clone(), path_prefix);
    if let Some(old) = diff.old_path {
        diff.old_path = Some(prefix_path(old, path_prefix));
    }
    if let Some(new) = diff.new_path {
        diff.new_path = Some(prefix_path(new, path_prefix));
    }
    diff.repo_id = Some(repo_id);

    let patch = ConversationPatch::add_diff(escape_json_pointer_segment(&prefixed_entry), diff);
    (raw_path, LogMsg::JsonPatch(patch))
}

pub fn apply_stream_omit_policy(diff: &mut Diff, sent_bytes: &Arc<AtomicUsize>, stats_only: bool) {
    if stats_only {
        omit_diff_contents(diff);
        return;
    }

    let mut size = 0usize;
    if let Some(ref s) = diff.old_content {
        size += s.len();
    }
    if let Some(ref s) = diff.new_content {
        size += s.len();
    }

    if size == 0 {
        return;
    }

    let current = sent_bytes.load(Ordering::Relaxed);
    if current.saturating_add(size) > MAX_CUMULATIVE_DIFF_BYTES {
        omit_diff_contents(diff);
    } else {
        let _ = sent_bytes.fetch_add(size, Ordering::Relaxed);
    }
}

fn omit_diff_contents(diff: &mut Diff) {
    if diff.additions.is_none()
        && diff.deletions.is_none()
        && (diff.old_content.is_some() || diff.new_content.is_some())
    {
        let old = diff.old_content.as_deref().unwrap_or("");
        let new = diff.new_content.as_deref().unwrap_or("");
        let (add, del) = diff::compute_line_change_counts(old, new);
        diff.additions = Some(add);
        diff.deletions = Some(del);
    }

    diff.old_content = None;
    diff.new_content = None;
    diff.content_omitted = true;
}

fn extract_changed_paths(
    events: &[DebouncedEvent],
    canonical_worktree_path: &Path,
    worktree_path: &Path,
) -> Vec<String> {
    events
        .iter()
        .flat_map(|event| &event.paths)
        .filter_map(|path| {
            path.strip_prefix(canonical_worktree_path)
                .or_else(|_| path.strip_prefix(worktree_path))
                .ok()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn process_file_changes(
    git_service: &GitService,
    worktree_path: &Path,
    base_commit: &Commit,
    changed_paths: &[String],
    cumulative_bytes: &Arc<AtomicUsize>,
    full_sent_paths: &Arc<std::sync::RwLock<HashSet<String>>>,
    known_paths: &Arc<std::sync::RwLock<HashSet<String>>>,
    stats_only: bool,
    path_prefix: Option<&str>,
    repo_id: Uuid,
) -> Result<Vec<LogMsg>, DiffStreamError> {
    let path_filter: Vec<&str> = changed_paths.iter().map(|s| s.as_str()).collect();

    let current_diffs = git_service.get_diffs(
        DiffTarget::Worktree {
            worktree_path,
            base_commit,
        },
        Some(&path_filter),
    )?;

    let mut msgs = Vec::new();
    let mut files_with_diffs = HashSet::new();

    for mut diff in current_diffs {
        let raw_file_path = GitService::diff_path(&diff);
        files_with_diffs.insert(raw_file_path.clone());
        {
            let mut guard = known_paths.write().unwrap();
            guard.insert(raw_file_path.clone());
        }

        apply_stream_omit_policy(&mut diff, cumulative_bytes, stats_only);

        if diff.content_omitted {
            if full_sent_paths.read().unwrap().contains(&raw_file_path) {
                continue;
            }
        } else {
            let mut guard = full_sent_paths.write().unwrap();
            guard.insert(raw_file_path.clone());
        }

        let prefixed_entry_index = prefix_path(raw_file_path, path_prefix);
        if let Some(old) = diff.old_path {
            diff.old_path = Some(prefix_path(old, path_prefix));
        }
        if let Some(new) = diff.new_path {
            diff.new_path = Some(prefix_path(new, path_prefix));
        }
        diff.repo_id = Some(repo_id);

        let patch =
            ConversationPatch::add_diff(escape_json_pointer_segment(&prefixed_entry_index), diff);
        msgs.push(LogMsg::JsonPatch(patch));
    }

    for changed_path in changed_paths {
        if !files_with_diffs.contains(changed_path) {
            let prefixed_path = prefix_path(changed_path.clone(), path_prefix);
            let patch = ConversationPatch::remove_diff(escape_json_pointer_segment(&prefixed_path));
            msgs.push(LogMsg::JsonPatch(patch));
            {
                let mut guard = known_paths.write().unwrap();
                guard.remove(changed_path);
            }
        }
    }

    Ok(msgs)
}

/// Watches `.git/HEAD` and `.git/logs/HEAD` for changes.
/// Correctly resolves gitdir even for worktrees.
fn setup_git_watcher(
    git: &GitService,
    worktree_path: &Path,
) -> Option<(
    Debouncer<RecommendedWatcher, RecommendedCache>,
    tokio::sync::watch::Receiver<()>,
)> {
    let Ok(repo) = git.open_repo(worktree_path) else {
        tracing::warn!(
            "Failed to open git repo at {:?}, git events will be ignored",
            worktree_path
        );
        return None;
    };

    // For worktrees, repo.path() points to the actual gitdir (e.g. .git/worktrees/name or .git/)
    let gitdir = repo.path();
    let paths_to_watch = vec![gitdir.join("HEAD"), gitdir.join("logs").join("HEAD")];

    let (tx, rx) = tokio::sync::watch::channel(());

    // Create debouncer with short timeout since git operations might touch multiple files
    let mut debouncer = new_debouncer(
        Duration::from_millis(200),
        None,
        move |res: DebounceEventResult| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        },
    )
    .ok()?;

    let mut watched_any = false;
    for path in paths_to_watch {
        if path.exists() {
            if let Err(e) = debouncer.watch(&path, RecursiveMode::NonRecursive) {
                tracing::debug!("Failed to watch git path {:?}: {}", path, e);
            } else {
                watched_any = true;
            }
        }
    }

    if !watched_any {
        return None;
    }

    Some((debouncer, rx))
}
