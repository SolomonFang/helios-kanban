# Helios Kanban Remote

通过 REST API 远程控制正在运行的 [Helios Kanban](https://github.com/SolomonFang/vibe-kanban) 实例：列/建/改任务、启停 coding agent、跟进对话、查状态、处理审批。

面向能在本机执行 shell / HTTP 的聊天机器人（如手机上的 Hermes、Cursor、Claude Code）。同一台机器上更推荐 MCP；本技能用于**远程**场景。

## 功能

- 项目 / 仓库 / 分支：列出、创建项目、查看默认 executor
- 任务 CRUD：创建、筛选、更新、取消、删除；支持优先级、类型、迭代
- Agent 生命周期：启动（可多仓）、跟进、排队、停 workspace
- 审批：列出待审批并 approve / deny
- `@tag`：在描述和 follow-up 中展开为标签内容

不在范围内（请用桌面 Web UI）：创建/合并 PR、push、rebase、冲突解决、完整 diff。

## 安装

Agent 自助安装（克隆本仓库、拷到技能目录、配环境变量、验证连通）：

- 文档：[INSTALL.md](INSTALL.md)
- 给任意 agent 的一句话：

```
请阅读并执行安装文档，完成 helios-kanban-remote 技能安装：
https://github.com/SolomonFang/vibe-kanban/blob/hly-dev/skills/helios-kanban-remote/INSTALL.md
```

依赖：`curl`、`jq`。Kanban 服务需可达（建议 Tailscale）：

```bash
HOST=0.0.0.0 PORT=7964 npx helios-kanban
```

## 配置

| 变量 | 说明 |
|------|------|
| `HELIOS_KANBAN_URL` | **必填**，例如 `http://100.x.x.x:7964` |
| `HELIOS_KANBAN_PROJECT_ID` | 默认项目 UUID |
| `HELIOS_KANBAN_REPO_ID` | 默认仓库 UUID |
| `HELIOS_KANBAN_ITERATION` | 默认迭代，例如 `260717` |

## 快速使用

```bash
export HELIOS_KANBAN_URL="http://100.x.x.x:7964"
HK="bash scripts/hk.sh"

$HK health
$HK projects
$HK tasks list --status inprogress
$HK create-and-start "修复登录" --branch develop --desc "登录页 500 @coding-standards"
$HK status <task_id>
$HK follow-up <task_id> 请补充单元测试
```

完整命令与对话映射见 [SKILL.md](SKILL.md)；场景示例见 [examples.md](examples.md)；REST 端点见 [reference.md](reference.md)。

```bash
bash scripts/hk.sh --help
```

## 目录

```
helios-kanban-remote/
  SKILL.md          # Agent 使用说明（何时用、命令映射、安全规则）
  INSTALL.md        # Agent 自助安装
  reference.md      # REST API
  examples.md       # 端到端示例
  scripts/hk.sh     # CLI（curl + jq）
```

## MCP（同机）vs 本技能（远程）

同机跑 Kanban 时优先 MCP，覆盖完整编排面。远程（手机 bot、另一台机器）用 `hk.sh`。能力先上 MCP，再镜像到 CLI；不一致时以 [reference.md](reference.md) 的 REST API 为准。

```json
{
  "helios_kanban": {
    "command": "npx",
    "args": ["-y", "helios-kanban@latest", "--mcp"]
  }
}
```

## 许可

Apache License 2.0。详见仓库根目录 [LICENSE](../../LICENSE)。
