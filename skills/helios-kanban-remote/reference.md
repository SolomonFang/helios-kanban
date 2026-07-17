# Helios Kanban REST API Reference

Base URL: `$HELIOS_KANBAN_URL` (no trailing slash). All paths under `/api`.

Response envelope:

```json
{ "success": true, "data": { ... }, "message": null, "error_data": null }
```

Read `.data` on success; read `.message` on failure.

## Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| GET | `/health` | Liveness check |
| GET | `/projects` | List projects |
| GET | `/projects/{project_id}/repositories` | List repos |
| GET | `/tasks?project_id={uuid}` | List tasks |
| POST | `/tasks` | Create task |
| GET | `/tasks/{task_id}` | Get task |
| PUT | `/tasks/{task_id}` | Update task |
| DELETE | `/tasks/{task_id}` | Delete task |
| POST | `/tasks/create-and-start` | Create task + start agent |
| POST | `/task-attempts` | Start agent on existing task |
| GET | `/task-attempts?task_id={uuid}` | List workspaces for task |
| POST | `/task-attempts/{workspace_id}/stop` | Stop running agent |

## Create task

```json
POST /api/tasks
{
  "project_id": "uuid",
  "title": "Fix login bug",
  "description": "Optional details",
  "status": "todo"
}
```

## Update task

```json
PUT /api/tasks/{task_id}
{
  "status": "inprogress",
  "title": "New title",
  "description": "Updated desc"
}
```

## Start agent (existing task)

```json
POST /api/task-attempts
{
  "task_id": "uuid",
  "executor_profile_id": {
    "executor": "CLAUDE_CODE",
    "variant": "DEFAULT"
  },
  "repos": [
    { "repo_id": "uuid", "target_branch": "main" }
  ]
}
```

## Create and start (one shot)

```json
POST /api/tasks/create-and-start
{
  "task": {
    "project_id": "uuid",
    "title": "Fix login bug",
    "description": "..."
  },
  "executor_profile_id": {
    "executor": "CLAUDE_CODE",
    "variant": null
  },
  "repos": [
    { "repo_id": "uuid", "target_branch": "main" }
  ]
}
```

## Executors (from default_profiles.json)

`CLAUDE_CODE`, `AMP`, `GEMINI`, `CODEX`, `COPILOT`, `REASONIX`, `CURSOR_AGENT`, `QWEN_CODE`, `OPENCODE`, `ECHO`, and others.

Common variants: `DEFAULT`, `PLAN`, `OPUS`, `APPROVALS`, `FLASH`, `PRO`, `HIGH`, `MAX`.

## Origin header

If accessing via a custom domain behind reverse proxy, server must have `VK_ALLOWED_ORIGINS` set. Direct IP/Tailscale access typically works without it.
