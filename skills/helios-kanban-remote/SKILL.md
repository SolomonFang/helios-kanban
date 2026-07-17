---
name: helios-kanban-remote
description: >-
  Remote control Helios Kanban (helios-kanban) via REST API — list/create/update
  tasks, start/stop coding agents, check status. Use when the user wants to
  control kanban from phone/chat, mentions Helios Kanban / helios-kanban /
  vibe-kanban remotely, or asks to create tasks, start agents, or check progress
  on a running instance. If the skill is not installed yet, read and follow
  INSTALL.md to self-install from GitHub first.
---

# Helios Kanban Remote Control

Control a running [Helios Kanban](https://github.com/SolomonFang/vibe-kanban) instance over HTTP. Designed for chat bots (e.g. Hermes on phone) that can run shell commands or HTTP requests on a host with network access to the kanban server.

## Prerequisites

1. **Helios Kanban running** on a reachable host:
   ```bash
   HOST=0.0.0.0 PORT=7964 npx helios-kanban
   ```
2. **Network**: bot host can reach the server (Tailscale recommended).
3. **Env vars** (set on the bot host):
   - `HELIOS_KANBAN_URL` — base URL, e.g. `http://100.x.x.x:7964`
   - `HELIOS_KANBAN_PROJECT_ID` — optional default project UUID

## Quick workflow

When the user asks in natural language, translate to API calls:

| User intent | Action |
|-------------|--------|
| "有哪些项目" | `hk projects` |
| "看看进行中的任务" | `hk tasks list $PROJECT --status inprogress` |
| "创建一个任务：修复登录 bug" | `hk tasks create $PROJECT "修复登录 bug"` |
| "用 Claude 跑这个任务" | `hk start <task_id> --executor CLAUDE_CODE --repo <repo_id>` |
| "新建并立即启动" | `hk create-and-start $PROJECT "标题" --executor CLAUDE_CODE --repo <repo_id>` |
| "停掉当前 agent" | find workspace → `hk stop <workspace_id>` |
| "任务详情" | `hk tasks get <task_id>` |
| "标记完成" | `hk tasks update <task_id> --status done` |

**Always** run `hk projects` first if `HELIOS_KANBAN_PROJECT_ID` is unset. Cache `project_id` and `repo_id` in the conversation.

## Use the CLI script (preferred)

From this skill directory:

```bash
bash scripts/hk.sh <command> [args]
```

Requires `curl` and `jq`. See `scripts/hk.sh --help`.

## Response format for chat

Reply concisely in the user's language. After each action, summarize:

```markdown
**项目**: {name}
**任务**: {title} (`{id}`)
**状态**: {status} | agent 运行中: {yes/no}
**下一步**: {suggestion}
```

On error, show the API `message` and suggest checking `HELIOS_KANBAN_URL` or running `hk health`.

## Executor names

Map user-friendly names to API values:

| User says | `--executor` value |
|-----------|-------------------|
| Claude / Claude Code | `CLAUDE_CODE` |
| Codex | `CODEX` |
| Gemini | `GEMINI` |
| Amp | `AMP` |
| Reasonix | `REASONIX` |
| Cursor | `CURSOR_AGENT` |
| Copilot | `COPILOT` |

Optional `--variant`: `DEFAULT`, `PLAN`, `OPUS`, etc. (see `reference.md`).

## Task statuses

`todo` | `inprogress` | `inreview` | `done` | `cancelled`

## Safety rules

1. **Confirm before destructive ops** — deleting tasks, force push, merge (not exposed in CLI; use Web UI).
2. **Don't expose kanban to public internet** without auth; prefer Tailscale.
3. **Phone = dispatch only** — tell user to review diffs on desktop before merge.
4. If `hk health` fails, stop and report connectivity issue.

## Not installed yet?

**Read and execute [INSTALL.md](INSTALL.md) first.** The skill lives in this repo at `skills/helios-kanban-remote/`.

Give any agent:

```
请阅读并执行安装文档：
https://github.com/SolomonFang/vibe-kanban/blob/hly-dev/skills/helios-kanban-remote/INSTALL.md
```

Or simply: `安装 Helios Kanban 远程控制技能：https://github.com/SolomonFang/vibe-kanban`

## MCP alternative (same machine only)

If the agent runs **on the same host** as Helios Kanban and supports MCP:

```json
{
  "helios_kanban": {
    "command": "npx",
    "args": ["-y", "helios-kanban@latest", "--mcp"]
  }
}
```

MCP tools: `list_projects`, `list_tasks`, `create_task`, `start_workspace_session`, `get_task`, `update_task`, `delete_task`. Prefer MCP when co-located; use HTTP/`hk.sh` for remote phone control.

## More detail

- API reference: [reference.md](reference.md)
- Examples: [examples.md](examples.md)
