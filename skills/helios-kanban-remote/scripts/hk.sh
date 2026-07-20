#!/usr/bin/env bash
# Helios Kanban remote API CLI — used by helios-kanban-remote skill
set -euo pipefail

BASE_URL="${HELIOS_KANBAN_URL:-http://127.0.0.1:7964}"
BASE_URL="${BASE_URL%/}"

require_jq() {
  command -v jq >/dev/null 2>&1 || {
    echo "error: jq is required" >&2
    exit 1
  }
}

api() {
  local method="$1"
  local path="$2"
  shift 2
  local url="${BASE_URL}/api${path}"
  local response
  local http_code

  response=$(curl -sS -w "\n%{http_code}" -X "$method" \
    -H "Content-Type: application/json" \
    "$@" "$url")
  http_code=$(echo "$response" | tail -n1)
  response=$(echo "$response" | sed '$d')

  if [[ "$http_code" -ge 400 ]]; then
    echo "HTTP $http_code: $response" >&2
    exit 1
  fi

  if ! echo "$response" | jq -e '.success == true' >/dev/null 2>&1; then
    local msg
    msg=$(echo "$response" | jq -r '.message // "unknown error"')
    echo "API error: $msg" >&2
    echo "$response" >&2
    exit 1
  fi

  echo "$response" | jq '.data'
}

task_url() {
  local project_id="$1"
  local task_id="$2"
  echo "${BASE_URL}/local-projects/${project_id}/tasks/${task_id}"
}

# If first arg looks like a UUID use it; else fall back to HELIOS_KANBAN_PROJECT_ID.
resolve_project_id() {
  local maybe="$1"
  if [[ "$maybe" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    echo "$maybe"
    return
  fi
  if [[ -n "${HELIOS_KANBAN_PROJECT_ID:-}" ]]; then
    echo "$HELIOS_KANBAN_PROJECT_ID"
    return
  fi
  echo ""
}

normalize_executor() {
  # Avoid ${var^^} — not supported on macOS system bash 3.2
  local raw
  raw=$(printf '%s' "$1" | tr '[:lower:]' '[:upper:]')
  raw="${raw//-/_}"
  case "$raw" in
    CLAUDE | CLAUDE_CODE) echo "CLAUDE_CODE" ;;
    CODEX) echo "CODEX" ;;
    GEMINI) echo "GEMINI" ;;
    AMP) echo "AMP" ;;
    REASONIX) echo "REASONIX" ;;
    CURSOR | CURSOR_AGENT) echo "CURSOR_AGENT" ;;
    COPILOT) echo "COPILOT" ;;
    QWEN | QWEN_CODE) echo "QWEN_CODE" ;;
    OPENCODE) echo "OPENCODE" ;;
    DROID) echo "DROID" ;;
    KIMI | KIMI_CLI) echo "KIMI_CLI" ;;
    *) echo "$raw" ;;
  esac
}

resolve_executor_profile() {
  local executor="${1:-}"
  local variant="${2:-}"

  if [[ -z "$executor" ]]; then
    local info
    info=$(api GET "/info")
    executor=$(echo "$info" | jq -r '.config.executor_profile.executor // empty')
    if [[ -z "$variant" ]]; then
      variant=$(echo "$info" | jq -r '.config.executor_profile.variant // empty')
    fi
    if [[ -z "$executor" ]]; then
      echo "error: could not read default executor from /api/info (config.executor_profile)" >&2
      exit 1
    fi
  else
    executor=$(normalize_executor "$executor")
  fi

  RESOLVED_EXECUTOR="$executor"
  RESOLVED_VARIANT="$variant"
}

resolve_target_branch() {
  local repo_id="$1"
  local branch="${2:-}"

  if [[ -n "$branch" ]]; then
    RESOLVED_BRANCH="$branch"
    return
  fi

  local repo
  repo=$(api GET "/repos/${repo_id}")
  branch=$(echo "$repo" | jq -r '.default_target_branch // empty')
  if [[ -z "$branch" ]]; then
    branch="main"
  fi
  RESOLVED_BRANCH="$branch"
}

resolve_repo_id() {
  local repo_id="${1:-}"
  if [[ -z "$repo_id" ]]; then
    repo_id="${HELIOS_KANBAN_REPO_ID:-}"
  fi
  if [[ -z "$repo_id" ]]; then
    echo "error: --repo is required (or set HELIOS_KANBAN_REPO_ID)" >&2
    exit 1
  fi
  echo "$repo_id"
}

resolve_iteration() {
  local iteration="${1:-}"
  if [[ -z "$iteration" ]]; then
    iteration="${HELIOS_KANBAN_ITERATION:-}"
  fi
  echo "$iteration"
}

# Expand @tagname in text via GET /api/tags (same behavior as MCP create_task).
expand_tags() {
  local text="$1"
  if [[ "$text" != *@* ]]; then
    printf '%s' "$text"
    return
  fi
  local tags
  tags=$(api GET "/tags" 2>/dev/null || echo "[]")
  if ! echo "$tags" | jq -e 'type == "array" and length > 0' >/dev/null 2>&1; then
    printf '%s' "$text"
    return
  fi
  jq -n --arg text "$text" --argjson tags "$tags" '
    reduce $tags[] as $t ($text; gsub("@\($t.tag_name)"; $t.content))
  ' -r
}

# Build repos JSON array for start/create-and-start.
# Inputs: REPO_SPECS (bash array of "uuid" or "uuid:branch"), optional GLOBAL_BRANCH.
# Each repo without an explicit branch uses default_target_branch (else GLOBAL_BRANCH/main).
build_repos_json() {
  local global_branch="${1:-}"
  shift
  local specs=("$@")
  if [[ ${#specs[@]} -eq 0 ]]; then
    local default_repo
    default_repo="${HELIOS_KANBAN_REPO_ID:-}"
    if [[ -z "$default_repo" ]]; then
      echo "error: --repo is required (or set HELIOS_KANBAN_REPO_ID)" >&2
      exit 1
    fi
    specs=("$default_repo")
  fi

  local arr="[]"
  local spec repo_id branch
  for spec in "${specs[@]}"; do
    if [[ "$spec" == *:* ]]; then
      repo_id="${spec%%:*}"
      branch="${spec#*:}"
    else
      repo_id="$spec"
      branch=""
    fi
    if [[ -z "$branch" ]]; then
      if [[ -n "$global_branch" ]]; then
        branch="$global_branch"
      else
        resolve_target_branch "$repo_id" ""
        branch="$RESOLVED_BRANCH"
      fi
    fi
    arr=$(jq -n --argjson a "$arr" --arg rid "$repo_id" --arg br "$branch" \
      '$a + [{repo_id: $rid, target_branch: $br}]')
  done
  REPOS_JSON="$arr"
}

# Resolve latest session for a task_id (preferred) or workspace_id.
# Sets RESOLVED_SESSION_ID, RESOLVED_WORKSPACE_ID, RESOLVED_TASK_ID, RESOLVED_SESSION_EXECUTOR
resolve_latest_session_for() {
  local id="$1"
  local workspaces sessions ws_id session_id session_exec

  workspaces=$(api GET "/task-attempts?task_id=${id}" 2>/dev/null || echo "[]")
  if echo "$workspaces" | jq -e 'type == "array" and length > 0' >/dev/null 2>&1; then
    ws_id=$(echo "$workspaces" | jq -r '.[0].id')
    RESOLVED_TASK_ID="$id"
  else
    # Treat id as workspace_id
    ws_id="$id"
    RESOLVED_TASK_ID=$(api GET "/task-attempts" | jq -r --arg w "$ws_id" \
      '[.[] | select(.id == $w)][0].task_id // empty')
  fi

  sessions=$(api GET "/sessions?workspace_id=${ws_id}")
  if ! echo "$sessions" | jq -e 'type == "array" and length > 0' >/dev/null 2>&1; then
    echo "error: no sessions found for workspace ${ws_id}" >&2
    exit 1
  fi

  session_id=$(echo "$sessions" | jq -r '.[0].id')
  session_exec=$(echo "$sessions" | jq -r '.[0].executor // empty')
  RESOLVED_WORKSPACE_ID="$ws_id"
  RESOLVED_SESSION_ID="$session_id"
  RESOLVED_SESSION_EXECUTOR="$session_exec"
}

usage() {
  cat <<'EOF'
Helios Kanban remote CLI

Env:
  HELIOS_KANBAN_URL          Base URL (default: http://127.0.0.1:7964)
  HELIOS_KANBAN_PROJECT_ID   Default project UUID (omit project_id args when set)
  HELIOS_KANBAN_REPO_ID      Default repo UUID (omit --repo when set)
  HELIOS_KANBAN_ITERATION    Default iteration code (e.g. 260717)

Commands:
  health
  info
  projects
  projects update <project_id> [--name TEXT] [--description TEXT]
  repos [project_id]
  branches <repo_id> [--query TEXT]
  tasks list [project_id] [--status S] [--iteration CODE] [--query TEXT] [--limit N]
  tasks get <task_id>
  tasks create [project_id] <title> [--desc TEXT] [--iteration CODE]
  tasks update <task_id> [--title T] [--status S] [--desc T] [--iteration CODE]
  tasks cancel <task_id>
  tasks delete <task_id>
  start <task_id> [--repo ID|ID:branch]... [--executor E] [--variant V] [--branch B]
  create-and-start [project_id] <title> [--repo ID|ID:branch]... [--executor E] [--variant V] [--branch B] [--desc T] [--iteration CODE]
  follow-up <task_id|workspace_id> <prompt...>   # auto-queues if agent running; expands @tags
  status <task_id>
  workspaces [--task TASK_ID]
  stop <workspace_id>
  tags
  approvals
  approve <approval_id> --process <execution_process_id>
  deny <approval_id> --process <execution_process_id> [--reason TEXT]

Notes:
  --executor optional → Settings default (config.executor_profile)
  --branch optional → repo.default_target_branch, else main
  --repo may repeat; use ID:branch for per-repo base branch
  --iteration optional → HELIOS_KANBAN_ITERATION when unset
  @tagname in --desc / follow-up expands via /api/tags
  cancel ≠ delete ≠ stop (see SKILL.md)

Examples:
  hk tasks create "Fix login" --desc "Use @coding-standards"
  hk start <task_id> --repo <uuid1> --repo <uuid2>:develop
  hk follow-up <task_id> please also add unit tests
  hk status <task_id>
  hk approvals && hk approve <id> --process <ep_id>
  hk branches <repo_id> --query develop
EOF
}

cmd_health() {
  curl -sS "${BASE_URL}/api/health" | jq .
}

cmd_info() {
  require_jq
  api GET "/info"
}

cmd_projects() {
  require_jq
  if [[ "${1:-}" == "update" ]]; then
    shift
    cmd_projects_update "$@"
    return
  fi
  local projects
  projects=$(api GET "/projects")
  # Enrich with repo names for agent routing
  echo "$projects" | jq -c '.[]' | while read -r row; do
    local pid
    pid=$(echo "$row" | jq -r '.id')
    local repos
    repos=$(api GET "/projects/${pid}/repositories" 2>/dev/null || echo "[]")
    echo "$row" | jq --argjson repos "$repos" \
      '. + {repos: [$repos[] | (.display_name // .name)], description: (.description // null)}'
  done | jq -s '.'
}

cmd_projects_update() {
  require_jq
  local project_id="$1"
  shift
  local name="" description="" has_description=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --name) name="$2"; shift 2 ;;
      --description) description="$2"; has_description=1; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local payload="{}"
  [[ -n "$name" ]] && payload=$(echo "$payload" | jq --arg v "$name" '. + {name: $v}')
  if [[ "$has_description" -eq 1 ]]; then
    payload=$(echo "$payload" | jq --arg v "$description" '. + {description: $v}')
  fi
  api PUT "/projects/${project_id}" -d "$payload"
}

cmd_repos() {
  require_jq
  local project_id
  project_id=$(resolve_project_id "${1:-}")
  if [[ -z "$project_id" ]]; then
    echo "error: project_id required (or set HELIOS_KANBAN_PROJECT_ID)" >&2
    exit 1
  fi
  api GET "/projects/${project_id}/repositories"
}

cmd_branches() {
  require_jq
  local repo_id="$1"
  shift
  local query=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --query) query="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local data
  data=$(api GET "/repos/${repo_id}/branches")
  if [[ -n "$query" ]]; then
    echo "$data" | jq --arg q "$query" \
      '[.[] | select(.name | test($q; "i"))] | map({name, is_current, is_remote})'
  else
    echo "$data" | jq 'map({name, is_current, is_remote})'
  fi
}

cmd_tasks_list() {
  require_jq
  local project_id=""
  if [[ $# -gt 0 && "$1" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    project_id="$1"
    shift
  else
    project_id="${HELIOS_KANBAN_PROJECT_ID:-}"
  fi
  if [[ -z "$project_id" ]]; then
    echo "error: project_id required (or set HELIOS_KANBAN_PROJECT_ID)" >&2
    exit 1
  fi

  local status="" iteration="" query="" limit="50"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --status) status="$2"; shift 2 ;;
      --iteration) iteration="$2"; shift 2 ;;
      --query) query="$2"; shift 2 ;;
      --limit) limit="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local data
  data=$(api GET "/tasks?project_id=${project_id}")
  local filter='.'
  if [[ -n "$status" ]]; then
    filter="$filter | map(select(.status == \$status))"
  fi
  if [[ -n "$iteration" ]]; then
    filter="$filter | map(select(.iteration == \$iteration))"
  fi
  if [[ -n "$query" ]]; then
    filter="$filter | map(select((.title + \" \" + (.description // \"\")) | test(\$query; \"i\")))"
  fi
  echo "$data" | jq --arg status "$status" --arg iteration "$iteration" --arg query "$query" \
    "$filter | .[0:$limit]"
}

cmd_tasks_get() {
  require_jq
  api GET "/tasks/$1"
}

cmd_tasks_create() {
  require_jq
  local project_id="" title=""
  if [[ $# -ge 2 && "$1" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    project_id="$1"
    title="$2"
    shift 2
  elif [[ $# -ge 1 ]]; then
    project_id="${HELIOS_KANBAN_PROJECT_ID:-}"
    title="$1"
    shift 1
  fi
  if [[ -z "$project_id" || -z "$title" ]]; then
    echo "error: need [project_id] <title> (or set HELIOS_KANBAN_PROJECT_ID)" >&2
    exit 1
  fi

  local desc="" iteration=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --desc) desc="$2"; shift 2 ;;
      --iteration) iteration="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  iteration=$(resolve_iteration "$iteration")
  if [[ -n "$desc" ]]; then
    desc=$(expand_tags "$desc")
  fi

  local payload task
  payload=$(jq -n \
    --arg pid "$project_id" --arg t "$title" --arg d "$desc" --arg it "$iteration" \
    '{
      project_id: $pid,
      title: $t,
      status: "todo"
    }
    + (if $d == "" then {} else {description: $d} end)
    + (if $it == "" then {} else {iteration: $it} end)')
  task=$(api POST "/tasks" -d "$payload")
  echo "$task" | jq --arg url "$(task_url "$project_id" "$(echo "$task" | jq -r '.id')")" \
    '. + {url: $url}'
}

cmd_tasks_update() {
  require_jq
  local task_id="$1"
  shift
  local title="" status="" desc="" iteration="" has_iteration=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --title) title="$2"; shift 2 ;;
      --status) status="$2"; shift 2 ;;
      --desc) desc="$2"; shift 2 ;;
      --iteration) iteration="$2"; has_iteration=1; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local payload="{}"
  [[ -n "$title" ]] && payload=$(echo "$payload" | jq --arg v "$title" '. + {title: $v}')
  [[ -n "$status" ]] && payload=$(echo "$payload" | jq --arg v "$status" '. + {status: $v}')
  if [[ -n "$desc" ]]; then
    desc=$(expand_tags "$desc")
    payload=$(echo "$payload" | jq --arg v "$desc" '. + {description: $v}')
  fi
  if [[ "$has_iteration" -eq 1 ]]; then
    payload=$(echo "$payload" | jq --arg v "$iteration" '. + {iteration: $v}')
  fi
  api PUT "/tasks/${task_id}" -d "$payload"
}

cmd_tasks_delete() {
  require_jq
  api DELETE "/tasks/$1"
}

cmd_tasks_cancel() {
  require_jq
  local task_id="$1"
  # Stop running workspaces first (best-effort)
  local workspaces
  workspaces=$(api GET "/task-attempts?task_id=${task_id}" 2>/dev/null || echo "[]")
  if echo "$workspaces" | jq -e 'type == "array"' >/dev/null 2>&1; then
    local ids
    ids=$(echo "$workspaces" | jq -r '.[].id')
    for wid in $ids; do
      api POST "/task-attempts/${wid}/stop" -d '{}' >/dev/null 2>&1 || true
    done
  fi
  local payload
  payload=$(jq -n '{status: "cancelled"}')
  api PUT "/tasks/${task_id}" -d "$payload"
}

cmd_start() {
  require_jq
  local task_id="$1"
  shift
  local executor="" variant="" branch=""
  local repo_specs=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --executor) executor="$2"; shift 2 ;;
      --variant) variant="$2"; shift 2 ;;
      --repo) repo_specs+=("$2"); shift 2 ;;
      --branch) branch="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  resolve_executor_profile "$executor" "$variant"
  build_repos_json "$branch" "${repo_specs[@]+"${repo_specs[@]}"}"
  local payload result project_id
  if [[ -n "$RESOLVED_VARIANT" ]]; then
    payload=$(jq -n \
      --arg tid "$task_id" --arg ex "$RESOLVED_EXECUTOR" --arg var "$RESOLVED_VARIANT" \
      --argjson repos "$REPOS_JSON" \
      '{task_id: $tid, executor_profile_id: {executor: $ex, variant: $var}, repos: $repos}')
  else
    payload=$(jq -n \
      --arg tid "$task_id" --arg ex "$RESOLVED_EXECUTOR" \
      --argjson repos "$REPOS_JSON" \
      '{task_id: $tid, executor_profile_id: {executor: $ex}, repos: $repos}')
  fi
  result=$(api POST "/task-attempts" -d "$payload")
  project_id=$(api GET "/tasks/${task_id}" | jq -r '.project_id')
  echo "$result" | jq \
    --arg url "$(task_url "$project_id" "$task_id")" \
    --argjson repos "$REPOS_JSON" \
    --arg executor "$RESOLVED_EXECUTOR" \
    '. + {url: $url, repos: $repos, executor: $executor}'
}

cmd_create_and_start() {
  require_jq
  local project_id="" title=""
  if [[ $# -ge 2 && "$1" =~ ^[0-9a-fA-F-]{36}$ ]]; then
    project_id="$1"
    title="$2"
    shift 2
  elif [[ $# -ge 1 ]]; then
    project_id="${HELIOS_KANBAN_PROJECT_ID:-}"
    title="$1"
    shift 1
  fi
  if [[ -z "$project_id" || -z "$title" ]]; then
    echo "error: need [project_id] <title> (or set HELIOS_KANBAN_PROJECT_ID)" >&2
    exit 1
  fi

  local executor="" variant="" branch="" desc="" iteration=""
  local repo_specs=()
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --executor) executor="$2"; shift 2 ;;
      --variant) variant="$2"; shift 2 ;;
      --repo) repo_specs+=("$2"); shift 2 ;;
      --branch) branch="$2"; shift 2 ;;
      --desc) desc="$2"; shift 2 ;;
      --iteration) iteration="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  iteration=$(resolve_iteration "$iteration")
  if [[ -n "$desc" ]]; then
    desc=$(expand_tags "$desc")
  fi
  resolve_executor_profile "$executor" "$variant"
  build_repos_json "$branch" "${repo_specs[@]+"${repo_specs[@]}"}"

  local task_obj payload result
  task_obj=$(jq -n \
    --arg pid "$project_id" --arg t "$title" --arg d "$desc" --arg it "$iteration" \
    '{
      project_id: $pid,
      title: $t
    }
    + (if $d == "" then {} else {description: $d} end)
    + (if $it == "" then {} else {iteration: $it} end)')
  if [[ -n "$RESOLVED_VARIANT" ]]; then
    payload=$(jq -n \
      --argjson task "$task_obj" --arg ex "$RESOLVED_EXECUTOR" --arg var "$RESOLVED_VARIANT" \
      --argjson repos "$REPOS_JSON" \
      '{task: $task, executor_profile_id: {executor: $ex, variant: $var}, repos: $repos}')
  else
    payload=$(jq -n \
      --argjson task "$task_obj" --arg ex "$RESOLVED_EXECUTOR" \
      --argjson repos "$REPOS_JSON" \
      '{task: $task, executor_profile_id: {executor: $ex}, repos: $repos}')
  fi
  result=$(api POST "/tasks/create-and-start" -d "$payload")
  local tid
  tid=$(echo "$result" | jq -r '.id // .task_id // empty')
  echo "$result" | jq \
    --arg url "$(task_url "$project_id" "$tid")" \
    --argjson repos "$REPOS_JSON" \
    --arg executor "$RESOLVED_EXECUTOR" \
    '. + {url: $url, repos: $repos, executor: $executor}'
}

cmd_follow_up() {
  require_jq
  if [[ $# -lt 2 ]]; then
    echo "usage: hk follow-up <task_id|workspace_id> <prompt...>" >&2
    exit 1
  fi
  local id="$1"
  shift
  local prompt="$*"
  if [[ -z "$prompt" ]]; then
    echo "error: prompt required" >&2
    exit 1
  fi
  prompt=$(expand_tags "$prompt")

  resolve_latest_session_for "$id"

  local executor="$RESOLVED_SESSION_EXECUTOR"
  local variant=""
  resolve_executor_profile "$executor" "$variant"

  local running="false"
  if [[ -n "${RESOLVED_TASK_ID:-}" ]]; then
    local task_row
    task_row=$(api GET "/tasks/${RESOLVED_TASK_ID}")
    local project_id
    project_id=$(echo "$task_row" | jq -r '.project_id')
    running=$(api GET "/tasks?project_id=${project_id}" \
      | jq -r --arg tid "$RESOLVED_TASK_ID" \
        '[.[] | select(.id == $tid)][0].has_in_progress_attempt // false')
  fi

  local payload result mode
  if [[ -n "$RESOLVED_VARIANT" ]]; then
    payload=$(jq -n \
      --arg prompt "$prompt" --arg ex "$RESOLVED_EXECUTOR" --arg var "$RESOLVED_VARIANT" \
      '{prompt: $prompt, executor_profile_id: {executor: $ex, variant: $var}}')
  else
    payload=$(jq -n \
      --arg prompt "$prompt" --arg ex "$RESOLVED_EXECUTOR" \
      '{prompt: $prompt, executor_profile_id: {executor: $ex}}')
  fi

  if [[ "$running" == "true" ]]; then
    mode="queued"
    local qpayload
    if [[ -n "$RESOLVED_VARIANT" ]]; then
      qpayload=$(jq -n \
        --arg message "$prompt" --arg ex "$RESOLVED_EXECUTOR" --arg var "$RESOLVED_VARIANT" \
        '{message: $message, executor_profile_id: {executor: $ex, variant: $var}}')
    else
      qpayload=$(jq -n \
        --arg message "$prompt" --arg ex "$RESOLVED_EXECUTOR" \
        '{message: $message, executor_profile_id: {executor: $ex}}')
    fi
    result=$(api POST "/sessions/${RESOLVED_SESSION_ID}/queue" -d "$qpayload")
  else
    mode="follow_up"
    result=$(api POST "/sessions/${RESOLVED_SESSION_ID}/follow-up" -d "$payload")
  fi

  echo "$result" | jq \
    --arg mode "$mode" \
    --arg session_id "$RESOLVED_SESSION_ID" \
    --arg workspace_id "$RESOLVED_WORKSPACE_ID" \
    '{mode: $mode, session_id: $session_id, workspace_id: $workspace_id, result: .}'
}

cmd_status() {
  require_jq
  local task_id="$1"
  local task workspaces summaries
  task=$(api GET "/tasks/${task_id}")
  workspaces=$(api GET "/task-attempts?task_id=${task_id}")
  summaries=$(api POST "/task-attempts/summary" -d '{"archived": false}' 2>/dev/null || echo '{"summaries":[]}')

  local project_id
  project_id=$(echo "$task" | jq -r '.project_id')
  local tasks_list
  tasks_list=$(api GET "/tasks?project_id=${project_id}")
  local attempt_flags
  attempt_flags=$(echo "$tasks_list" | jq --arg tid "$task_id" \
    '[.[] | select(.id == $tid)][0] | {has_in_progress_attempt, last_attempt_failed, executor}')

  echo "$task" | jq \
    --argjson workspaces "$workspaces" \
    --argjson summaries "$(echo "$summaries" | jq '.summaries // []')" \
    --argjson flags "$attempt_flags" \
    --arg url "$(task_url "$project_id" "$task_id")" \
    '{
      task: {
        id: .id,
        title: .title,
        status: .status,
        iteration: .iteration,
        description: .description
      },
      url: $url,
      running: ($flags.has_in_progress_attempt // false),
      last_attempt_failed: ($flags.last_attempt_failed // false),
      executor: ($flags.executor // null),
      workspaces: (
        $workspaces | map(. as $w | {
          id: $w.id,
          branch: $w.branch,
          archived: $w.archived,
          name: $w.name,
          summary: ($summaries | map(select(.workspace_id == $w.id))[0] // null)
        })
      )
    }'
}

cmd_workspaces() {
  require_jq
  local task_id=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --task) task_id="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  if [[ -n "$task_id" ]]; then
    api GET "/task-attempts?task_id=${task_id}"
  else
    api GET "/task-attempts"
  fi
}

cmd_stop() {
  require_jq
  api POST "/task-attempts/$1/stop" -d '{}'
}

cmd_tags() {
  require_jq
  api GET "/tags" | jq 'map({id, tag_name, content: (.content | .[0:120])})'
}

cmd_approvals() {
  require_jq
  api GET "/approvals"
}

cmd_approve() {
  require_jq
  local approval_id="$1"
  shift
  local process_id=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --process) process_id="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  [[ -z "$process_id" ]] && {
    echo "error: --process <execution_process_id> is required" >&2
    exit 1
  }
  local payload
  payload=$(jq -n --arg ep "$process_id" \
    '{execution_process_id: $ep, status: {status: "approved"}}')
  api POST "/approvals/${approval_id}/respond" -d "$payload"
}

cmd_deny() {
  require_jq
  local approval_id="$1"
  shift
  local process_id="" reason=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --process) process_id="$2"; shift 2 ;;
      --reason) reason="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  [[ -z "$process_id" ]] && {
    echo "error: --process <execution_process_id> is required" >&2
    exit 1
  }
  local payload
  if [[ -n "$reason" ]]; then
    payload=$(jq -n --arg ep "$process_id" --arg r "$reason" \
      '{execution_process_id: $ep, status: {status: "denied", reason: $r}}')
  else
    payload=$(jq -n --arg ep "$process_id" \
      '{execution_process_id: $ep, status: {status: "denied"}}')
  fi
  api POST "/approvals/${approval_id}/respond" -d "$payload"
}

# --- main ---
if [[ $# -eq 0 || "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

command="$1"
shift

case "$command" in
  health) cmd_health ;;
  info) cmd_info ;;
  projects) cmd_projects ;;
  repos) cmd_repos "$@" ;;
  branches) cmd_branches "$@" ;;
  tasks)
    sub="${1:-}"; shift || true
    case "$sub" in
      list) cmd_tasks_list "$@" ;;
      get) cmd_tasks_get "$@" ;;
      create) cmd_tasks_create "$@" ;;
      update) cmd_tasks_update "$@" ;;
      cancel) cmd_tasks_cancel "$@" ;;
      delete) cmd_tasks_delete "$@" ;;
      *) echo "unknown tasks subcommand: $sub" >&2; usage; exit 1 ;;
    esac
    ;;
  start) cmd_start "$@" ;;
  create-and-start) cmd_create_and_start "$@" ;;
  follow-up) cmd_follow_up "$@" ;;
  status) cmd_status "$@" ;;
  workspaces) cmd_workspaces "$@" ;;
  stop) cmd_stop "$@" ;;
  tags) cmd_tags ;;
  approvals) cmd_approvals ;;
  approve) cmd_approve "$@" ;;
  deny) cmd_deny "$@" ;;
  *) echo "unknown command: $command" >&2; usage; exit 1 ;;
esac
