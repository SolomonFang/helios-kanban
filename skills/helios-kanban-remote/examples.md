# Examples

## Setup on bot host (Tailscale)

```bash
export HELIOS_KANBAN_URL="http://100.64.0.5:7964"
export HELIOS_KANBAN_PROJECT_ID="a1b2c3d4-e5f6-7890-abcd-ef1234567890"
```

## Phone → Hermes conversation

**User**: 看看现在有哪些任务在进行？

**Agent**:
```bash
bash scripts/hk.sh tasks list "$HELIOS_KANBAN_PROJECT_ID" --status inprogress
```

**Reply**: 当前有 2 个进行中的任务：1) 修复登录 bug（agent 运行中）2) 添加单元测试（等待审查）

---

**User**: 帮我新建一个任务，让 Claude 去重构 auth 模块

**Agent**:
```bash
bash scripts/hk.sh repos "$HELIOS_KANBAN_PROJECT_ID"
bash scripts/hk.sh create-and-start "$HELIOS_KANBAN_PROJECT_ID" "重构 auth 模块" \
  --executor CLAUDE_CODE --repo "<repo_id>" --desc "重构认证模块，保持 API 兼容"
```

**Reply**: 已创建任务并启动 Claude Code，workspace ID: `...`。完成后请在电脑端审查 diff。

---

**User**: 停掉它

**Agent**:
```bash
bash scripts/hk.sh workspaces --task "<task_id>"
bash scripts/hk.sh stop "<workspace_id>"
```

## curl without script

```bash
curl -s "$HELIOS_KANBAN_URL/api/health" | jq .
curl -s "$HELIOS_KANBAN_URL/api/projects" | jq '.data'
```
