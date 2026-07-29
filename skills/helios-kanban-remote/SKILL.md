---
name: helios-kanban-remote
description: >-
  Remote control Helios Kanban (helios-kanban) via REST API — list/create/update
  tasks, start/stop coding agents, follow-up, check status, handle approvals.
  Use when the user wants to control kanban from phone/chat, mentions Helios
  Kanban / helios-kanban / vibe-kanban remotely, or asks to create tasks, start
  agents, or check progress on a running instance. If the skill is not installed
  yet, read and follow INSTALL.md to self-install from GitHub first.
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
   - `HELIOS_KANBAN_REPO_ID` — optional default repo UUID
   - `HELIOS_KANBAN_ITERATION` — optional default iteration (e.g. `260717`)

## Quick workflow

| User intent | Action |
|-------------|--------|
| "有哪些项目" | `hk projects`（看 `description` + `repos` 选项目） |
| "创建项目" | `hk projects create "名称" --repo-path /server/abs/path` |
| "默认 agent / 仓库" | `hk info`；`hk repos` |
| "看看进行中的" | `hk tasks list --status inprogress` |
| "找登录相关任务" | `hk tasks list --query 登录` |
| "260717 迭代" | `hk tasks list --iteration 260717` |
| "创建任务" | `hk tasks create "标题" --desc "用 @coding-standards"` |
| "紧急任务" | `hk tasks create "标题" --priority urgent` |
| "看紧急/高优任务" | `hk tasks list --priority urgent` |
| "有哪些分支" | `hk branches <repo_id> [--query develop]` |
| "多仓启动" | `hk start <task_id> --repo <id1> --repo <id2>:develop` |
| "有哪些 tag" | `hk tags` |
| "基于 develop 启动" | `hk start <task_id> --branch develop` |
| "新建并启动" | `hk create-and-start "标题"` |
| "再跟它说一句…" | `hk follow-up <task_id> <prompt>`（运行中自动排队） |
| "跑得怎么样了" | `hk status <task_id>` |
| "有没有待审批" | `hk approvals` → `hk approve/deny … --process <ep_id>` |
| "停掉 agent" | `hk workspaces --task <id>` → `hk stop <workspace_id>` |
| "取消任务" | `hk tasks cancel <task_id>`（会先 stop） |
| "删除任务" | 确认后 → `hk tasks delete <task_id>` |
| "标记完成" | `hk tasks update <task_id> --status done` |

Cache `project_id` / `repo_id` in the conversation. Prefer env defaults so commands stay short.

## Complete lifecycle

```text
1. hk repos / hk branches <repo>
2. hk tasks create "标题" [--iteration CODE] [--priority P]
3. hk start <task_id> [--branch B] [--repo R]
     └─ or: hk create-and-start "标题" …
4. hk status <task_id>                  # progress / diff summary
5. hk follow-up <task_id> <prompt>      # continue chatting
6. hk approvals → approve | deny
7a. hk stop <workspace_id>
7b. hk tasks cancel <task_id>
7c. hk tasks delete <task_id>           # confirm first
7d. hk tasks update --status done
```

### Defaults

| Flag | When omitted |
|------|----------------|
| `--executor` | Settings → `config.executor_profile` via `hk info` |
| `--branch` | `repo.default_target_branch`, else `main` |
| `--repo` | `HELIOS_KANBAN_REPO_ID` |
| `--iteration` | `HELIOS_KANBAN_ITERATION` |
| `project_id` arg | `HELIOS_KANBAN_PROJECT_ID` |

### cancel vs delete vs stop

| Intent | Command | Effect |
|--------|---------|--------|
| 停 agent | `hk stop` | Kills agent; task remains |
| 取消任务 | `hk tasks cancel` | Stops workspaces + status=`cancelled` |
| 删除任务 | `hk tasks delete` | Permanent — **confirm first** |

## Use the CLI

```bash
bash scripts/hk.sh <command> [args]
```

Requires `curl` and `jq`. See `scripts/hk.sh --help`.

## Response format for chat

```markdown
**项目**: {name}
**任务**: {title} (`{id}`)
**迭代**: {iteration or —}
**优先级**: {priority}
**状态**: {status} | running: {yes/no} | failed: {yes/no}
**分支**: {target_branch}
**Executor**: {executor}
**URL**: {url}
**下一步**: {suggestion}
```

## Executor names (only when user names one)

| User says | `--executor` |
|-----------|--------------|
| Claude / Claude Code | `CLAUDE_CODE` |
| Codex | `CODEX` |
| Gemini | `GEMINI` |
| Amp | `AMP` |
| Reasonix | `REASONIX` |
| Cursor | `CURSOR_AGENT` |
| Copilot | `COPILOT` |

## Task statuses

`todo` | `inprogress` | `inreview` | `done` | `cancelled`

## Task priorities

`urgent` | `high` | `medium` | `low` — default `medium` when omitted. Cards show a colored badge (red/orange/blue/gray).

## Safety rules

1. Confirm before `tasks delete` — prefer `cancel`.
2. Prefer Tailscale; don’t expose kanban publicly without auth.
3. Phone = dispatch / follow-up / approve — review diffs & merge on desktop.
4. If `hk health` fails, stop and report connectivity.
5. Multi-repo: repeat `--repo` (`ID` or `ID:branch`).
6. PR / push / merge / rebase: tell user to use desktop Web UI.
7. `@tagname` in `--desc` / follow-up expands via `hk tags` / `/api/tags`.

## Out of scope (point user to Web UI)

- Create/merge PR, push, rebase, conflict resolution
- Full diff viewer / open in editor
- Multi-repo workspace create

## Not installed yet?

**Read and execute [INSTALL.md](INSTALL.md) first.**

```
请阅读并执行安装文档：
https://github.com/SolomonFang/vibe-kanban/blob/hly-dev/skills/helios-kanban-remote/INSTALL.md
```

## MCP (same machine) vs this skill (remote)

- **Same host as the kanban server** → prefer the MCP server. It covers the full orchestration surface: project/task CRUD, `create_project`, `create_task_and_start`, `start/stop_workspace_session`, `follow_up_session`, `queue_message`, `get_task_status`, `list_approvals` / `respond_to_approval`, `list_branches`, `list_tags`.
- **Remote (phone bot, another host)** → use `hk.sh` over HTTP, as this skill documents.
- New capabilities land in the MCP server first; `hk.sh` mirrors them for remote use. If the two drift, the REST API in [reference.md](reference.md) is the source of truth.

```json
{
  "helios_kanban": {
    "command": "npx",
    "args": ["-y", "helios-kanban@latest", "--mcp"]
  }
}
```

MCP `start_workspace_session` / `create_task_and_start` may omit `executor` / `base_branch` (uses Settings + repo defaults).

## More detail

- API reference: [reference.md](reference.md)
- Examples: [examples.md](examples.md)
