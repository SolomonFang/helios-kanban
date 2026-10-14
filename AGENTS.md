# Repository Guidelines

## Project Overview

**Helios Kanban** is a kanban board for orchestrating AI coding agents (Claude Code, Gemini CLI, Codex, Amp, Kimi, Qwen, Copilot, Cursor, OpenCode, Droid, and others). It is a fork of [BloopAI/vibe-kanban](https://github.com/BloopAI/vibe-kanban), published to npm as [`helios-kanban`](https://www.npmjs.com/package/helios-kanban). It lets users switch between agents, run agents in parallel or in sequence on tasks, review diffs, start dev servers, track task status, centralise agent MCP configuration, and open projects over SSH when running on a remote server.

The product is a local-first web app: a Rust/Axum backend serves a React SPA and manages agent processes, git worktrees, and a SQLite database. It is distributed as an npm package (`npx helios-kanban`).

### Technology stack

- **Backend**: Rust workspace (`Cargo.toml`, edition 2024, resolver 3), Axum 0.8 HTTP/WebSocket server, Tokio, SQLx (SQLite, compile-time-checked queries via `.sqlx/`), `tracing` for logging, `git2` + a git CLI wrapper for version control, Sentry for error reporting. Entry point: `crates/server/src/main.rs` (`--bin server`).
- **Frontend**: React 18 + TypeScript, Vite, Tailwind CSS, Radix UI primitives, TanStack Query/DB/Form, zustand, react-router, i18next (i18n enforced by lint), xterm.js for terminals, Lexical for rich text. Code in `frontend/src`.
- **Shared types**: Rust structs derive `TS` (ts-rs) and are exported to `shared/types.ts` — the single source of truth for the API contract between frontend and backend.
- **Distribution**: `npx-cli/` npm wrapper package; release workflow publishes per-platform binary packages (`helios-kanban-<platform>` as `optionalDependencies`, zips at package root), with GitHub Release assets as fallback.
- **Tooling**: pnpm 10 (Node ≥ 18), cargo. CI pins Node 22, pnpm 10.13.1, Rust nightly-2026-02-12.

## Project Structure & Module Organization

- `crates/`: Rust workspace crates —
  - `server` — Axum API (`src/routes/`), middleware, MCP task server, and binaries (`src/bin/server` entry via `main.rs`, `generate_types`, `mcp_task_server`).
  - `db` — SQLx models (`src/models/`: projects, repos, tasks, workspaces, sessions, execution processes, tags, images, merges…) and SQLite migrations in `crates/db/migrations/`.
  - `services` — business logic (`src/services/`): container/workspace management, worktree manager, events, diffs, approvals, auth, git host integrations, PR monitor, file search, notifications, remote sync, etc.
  - `executors` — per-agent executor implementations (`src/executors/`: claude, codex, gemini, amp, kimi, qwen, copilot, cursor, opencode, droid, acp, …) plus approvals, MCP config, and log handling.
  - `deployment` / `local-deployment` — `Deployment` trait abstraction and the local implementation (process spawning, PTY, command execution).
  - `git` — git CLI helpers and validation; `utils` — shared utilities (assets, browser open, JWT, msg store, sentry, shell).
  - `api-types`, `review` — supporting crates.
  - `remote` — **excluded from the workspace** (own `Cargo.toml`/`Cargo.lock`, Postgres via SQLx, Docker-based, optional private `billing` crate); built/deployed via `remote:*` scripts.
- `frontend/`: React + TypeScript app (Vite, Tailwind). Source in `frontend/src`; dialogs live in `frontend/src/components/dialogs`. See `frontend/CLAUDE.md` for the new design-system rules (CSS variables scoped to `.new-design`, custom color/spacing tokens, view/container/ui component split).
- `remote-frontend/`: Frontend for the remote (cloud) deployment.
- `shared/`: Generated TypeScript types (`shared/types.ts`). **Do not edit directly.**
- `assets/`, `dev_assets_seed/`, `dev_assets/`: Packaged assets (scripts, sounds) and local dev assets (config, dev SQLite DBs, signing key).
- `npx-cli/`: Files published to the npm CLI package.
- `vendor/`: Vendored/patched crates (`agent-client-protocol*` with a patch for Kimi's `elicitation/create` reverse-RPC; patched via `[patch]` in the root `Cargo.toml`, which also patches tokio-tungstenite/tungstenite to codex's forks).
- `scripts/`: Dev helpers (`setup-dev-environment.js` for ports/assets, `prepare-db.js` for SQLx offline data, `check-i18n.sh`).
- `docs/`: Mintlify documentation site (MDX + `docs.json`). See `docs/AGENTS.md` for writing rules (British English, Mintlify components, frontmatter required).

## Build, Test, and Development Commands

- Install: `pnpm i`
- Run dev (frontend + backend, ports auto-assigned by `scripts/setup-dev-environment.js`): `pnpm run dev`
- Backend only (watch mode, `cargo watch`): `pnpm run backend:dev:watch`
- Frontend only (Vite): `pnpm run frontend:dev`
- Type checks: `pnpm run check` (frontend `tsc --noEmit` + backend `cargo check`); individually `pnpm run frontend:check` / `pnpm run backend:check`
- Lint: `pnpm run lint` (ESLint + `cargo clippy --workspace --all-targets --features qa-mode -- -D warnings`)
- Format: `pnpm run format` (`cargo fmt --all` + Prettier)
- Rust tests: `cargo test --workspace`
- Generate TS types from Rust: `pnpm run generate-types` (`generate-types:check` in CI)
- Prepare SQLx offline query data (after changing any SQLx query or migration): `pnpm run prepare-db` (`prepare-db:check` in CI)
- Remote crate equivalents: `pnpm run remote:generate-types`, `remote:prepare-db`; remote dev via Docker Compose: `pnpm run remote:dev`
- Local NPX build: `pnpm run build:npx` (`local-build.sh`) then `pnpm pack` in `npx-cli/`

## Managing Shared Types Between Rust and TypeScript

ts-rs derives TypeScript types from Rust structs/enums annotated with `#[derive(TS)]`. After changing shared Rust types, regenerate with `pnpm run generate-types`. **Never edit `shared/types.ts` manually** — register new types in `crates/server/src/bin/generate_types.rs` instead. The remote crate has its own generator (`crates/remote/src/bin/generate_types.rs`, run via `remote:generate-types`).

## Coding Style & Naming Conventions

- Rust: `rustfmt` enforced (`rustfmt.toml`); group imports by crate; snake_case modules, PascalCase types. Keep functions small; add `Debug`/`Serialize`/`Deserialize` derives where useful.
- TypeScript/React: ESLint + Prettier (2 spaces, single quotes, 80 cols). PascalCase components, camelCase vars/functions, kebab-case file names where practical. i18n is lint-enforced (`eslint-plugin-i18next`, checked by `scripts/check-i18n.sh` in CI) — route user-facing strings through i18next.
- Frontend architecture rules (see `frontend/CLAUDE.md`): view components stateless, containers manage state, `ui-new/` primitives use PascalCase filenames; new design system uses scoped CSS variables and tokens (`text-high/normal/low`, `bg-primary/secondary/panel`, `p-base`/`m-double`, etc.) instead of default Tailwind grays.

## Testing Instructions

- Rust: prefer unit tests alongside code (`#[cfg(test)]`); run `cargo test --workspace`. Add tests for new logic and edge cases.
- Frontend: no broad test suite; ensure `pnpm run check` and `pnpm run lint` pass. If adding runtime logic, include lightweight tests (e.g., Vitest) in the same directory.
- CI (`.github/workflows/test.yml`) runs, on every PR to `main`: frontend lint + i18n check + prettier check + build; `generate-types:check`; `prepare-db:check`; `cargo fmt --check` + clippy with `-D warnings`; backend tests; and for the remote crate: lockfile freshness, `remote:generate-types:check`, `remote:prepare-db:check`. Make sure all of these pass locally before opening a PR.

## Deployment & Release

- Releases are driven by GitHub Actions (`.github/workflows/`): `release.yml` publishes the npm wrapper plus per-platform binary packages; `publish.yml` builds release binaries; `remote-deploy-*.yml` deploy the remote (cloud) stack.
- Release profile is size-optimised (`opt-level = "z"`, fat LTO, single codegen unit, stripped binaries) because binaries ship via npm platform packages.

## Security & Config Tips

- Use `.env` for local overrides; never commit secrets. Copy `.env.example` → `.env`. Set `FRONTEND_PORT` / `BACKEND_PORT` for fixed dev ports (browser localStorage/cache are tied to the origin).
- Dev ports and assets are managed by `scripts/setup-dev-environment.js` (prefers `.env` / `.dev-ports.json`; if a port is busy, bumps both ports by +1 until free).
- CORS in dev is controlled by `VK_ALLOWED_ORIGINS` (must match the URL you open in the browser).
- Production `npx helios-kanban`: set `PORT=7964` (avoid `PORT=0`, which assigns a random 5-digit port). `HOST` defaults to `127.0.0.1`.
- `dev_assets/` contains a local dev database and an Ed25519 signing key for development only — do not use these values in production or commit real credentials anywhere.
