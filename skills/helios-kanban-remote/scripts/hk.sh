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

normalize_executor() {
  local raw="${1^^}"
  raw="${raw//-/_}"
  case "$raw" in
    CLAUDE | CLAUDE_CODE) echo "CLAUDE_CODE" ;;
    CODEX) echo "CODEX" ;;
    GEMINI) echo "GEMINI" ;;
    AMP) echo "AMP" ;;
    REASONIX) echo "REASONIX" ;;
    CURSOR | CURSOR_AGENT) echo "CURSOR_AGENT" ;;
    COPILOT) echo "COPILOT" ;;
    *) echo "$raw" ;;
  esac
}

usage() {
  cat <<'EOF'
Helios Kanban remote CLI

Env:
  HELIOS_KANBAN_URL          Base URL (default: http://127.0.0.1:7964)
  HELIOS_KANBAN_PROJECT_ID   Default project UUID

Commands:
  health
  projects
  repos <project_id>
  tasks list <project_id> [--status STATUS] [--limit N]
  tasks get <task_id>
  tasks create <project_id> <title> [--desc TEXT]
  tasks update <task_id> [--title TEXT] [--status STATUS] [--desc TEXT]
  tasks delete <task_id>
  start <task_id> --executor EXEC [--variant VAR] --repo REPO_ID [--branch BRANCH]
  create-and-start <project_id> <title> --executor EXEC --repo REPO_ID [--branch main] [--desc TEXT]
  workspaces [--task TASK_ID]
  stop <workspace_id>

Examples:
  hk health
  hk projects
  hk tasks list $HELIOS_KANBAN_PROJECT_ID --status inprogress
  hk create-and-start $HELIOS_KANBAN_PROJECT_ID "Fix bug" --executor CLAUDE_CODE --repo <uuid>
EOF
}

cmd_health() {
  curl -sS "${BASE_URL}/api/health" | jq .
}

cmd_projects() {
  require_jq
  api GET "/projects"
}

cmd_repos() {
  require_jq
  local project_id="$1"
  api GET "/projects/${project_id}/repositories"
}

cmd_tasks_list() {
  require_jq
  local project_id="$1"
  shift
  local status=""
  local limit="50"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --status) status="$2"; shift 2 ;;
      --limit) limit="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local data
  data=$(api GET "/tasks?project_id=${project_id}")
  if [[ -n "$status" ]]; then
    echo "$data" | jq --arg s "$status" '[.[] | select(.status == $s)] | .[0:'"$limit"']'
  else
    echo "$data" | jq '.[0:'"$limit"']'
  fi
}

cmd_tasks_get() {
  require_jq
  api GET "/tasks/$1"
}

cmd_tasks_create() {
  require_jq
  local project_id="$1"
  local title="$2"
  shift 2
  local desc=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --desc) desc="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local payload
  if [[ -n "$desc" ]]; then
    payload=$(jq -n \
      --arg pid "$project_id" --arg t "$title" --arg d "$desc" \
      '{project_id: $pid, title: $t, description: $d, status: "todo"}')
  else
    payload=$(jq -n \
      --arg pid "$project_id" --arg t "$title" \
      '{project_id: $pid, title: $t, status: "todo"}')
  fi
  api POST "/tasks" -d "$payload"
}

cmd_tasks_update() {
  require_jq
  local task_id="$1"
  shift
  local title="" status="" desc=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --title) title="$2"; shift 2 ;;
      --status) status="$2"; shift 2 ;;
      --desc) desc="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  local payload="{}"
  [[ -n "$title" ]] && payload=$(echo "$payload" | jq --arg v "$title" '. + {title: $v}')
  [[ -n "$status" ]] && payload=$(echo "$payload" | jq --arg v "$status" '. + {status: $v}')
  [[ -n "$desc" ]] && payload=$(echo "$payload" | jq --arg v "$desc" '. + {description: $v}')
  api PUT "/tasks/${task_id}" -d "$payload"
}

cmd_tasks_delete() {
  require_jq
  api DELETE "/tasks/$1"
}

cmd_start() {
  require_jq
  local task_id="$1"
  shift
  local executor="" variant="" repo_id="" branch="main"
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --executor) executor="$2"; shift 2 ;;
      --variant) variant="$2"; shift 2 ;;
      --repo) repo_id="$2"; shift 2 ;;
      --branch) branch="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  [[ -z "$executor" || -z "$repo_id" ]] && {
    echo "error: --executor and --repo are required" >&2
    exit 1
  }
  executor=$(normalize_executor "$executor")
  local payload
  if [[ -n "$variant" ]]; then
    payload=$(jq -n \
      --arg tid "$task_id" --arg ex "$executor" --arg var "$variant" \
      --arg rid "$repo_id" --arg br "$branch" \
      '{task_id: $tid, executor_profile_id: {executor: $ex, variant: $var},
        repos: [{repo_id: $rid, target_branch: $br}]}')
  else
    payload=$(jq -n \
      --arg tid "$task_id" --arg ex "$executor" \
      --arg rid "$repo_id" --arg br "$branch" \
      '{task_id: $tid, executor_profile_id: {executor: $ex},
        repos: [{repo_id: $rid, target_branch: $br}]}')
  fi
  api POST "/task-attempts" -d "$payload"
}

cmd_create_and_start() {
  require_jq
  local project_id="$1"
  local title="$2"
  shift 2
  local executor="" variant="" repo_id="" branch="main" desc=""
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --executor) executor="$2"; shift 2 ;;
      --variant) variant="$2"; shift 2 ;;
      --repo) repo_id="$2"; shift 2 ;;
      --branch) branch="$2"; shift 2 ;;
      --desc) desc="$2"; shift 2 ;;
      *) echo "unknown arg: $1" >&2; exit 1 ;;
    esac
  done
  [[ -z "$executor" || -z "$repo_id" ]] && {
    echo "error: --executor and --repo are required" >&2
    exit 1
  }
  executor=$(normalize_executor "$executor")
  local task_obj
  if [[ -n "$desc" ]]; then
    task_obj=$(jq -n --arg pid "$project_id" --arg t "$title" --arg d "$desc" \
      '{project_id: $pid, title: $t, description: $d}')
  else
    task_obj=$(jq -n --arg pid "$project_id" --arg t "$title" \
      '{project_id: $pid, title: $t}')
  fi
  local payload
  if [[ -n "$variant" ]]; then
    payload=$(jq -n \
      --argjson task "$task_obj" --arg ex "$executor" --arg var "$variant" \
      --arg rid "$repo_id" --arg br "$branch" \
      '{task: $task, executor_profile_id: {executor: $ex, variant: $var},
        repos: [{repo_id: $rid, target_branch: $br}]}')
  else
    payload=$(jq -n \
      --argjson task "$task_obj" --arg ex "$executor" \
      --arg rid "$repo_id" --arg br "$branch" \
      '{task: $task, executor_profile_id: {executor: $ex},
        repos: [{repo_id: $rid, target_branch: $br}]}')
  fi
  api POST "/tasks/create-and-start" -d "$payload"
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

# --- main ---
if [[ $# -eq 0 || "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi

command="$1"
shift

case "$command" in
  health) cmd_health ;;
  projects) cmd_projects ;;
  repos) cmd_repos "$@" ;;
  tasks)
    sub="${1:-}"; shift || true
    case "$sub" in
      list) cmd_tasks_list "$@" ;;
      get) cmd_tasks_get "$@" ;;
      create) cmd_tasks_create "$@" ;;
      update) cmd_tasks_update "$@" ;;
      delete) cmd_tasks_delete "$@" ;;
      *) echo "unknown tasks subcommand: $sub" >&2; usage; exit 1 ;;
    esac
    ;;
  start) cmd_start "$@" ;;
  create-and-start) cmd_create_and_start "$@" ;;
  workspaces) cmd_workspaces "$@" ;;
  stop) cmd_stop "$@" ;;
  *) echo "unknown command: $command" >&2; usage; exit 1 ;;
esac
