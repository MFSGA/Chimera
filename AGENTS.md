# AGENTS

## Project overview

- Chimera is a Tauri desktop app with a Rust backend and a React/Vite frontend.
- The repo is a pnpm workspace (`frontend/*`, `scripts`).

## Key paths

- `backend/` Rust workspace; Tauri config lives in `backend/tauri/tauri.conf.json`.
- `frontend/chimera/` main app package (`chimera-ui`).
- `frontend/ui/` shared UI components.
- `frontend/interface/` shared interface/types package.
- `docs/` design notes and change logs.

## Setup

- During development, try to run with administrator/root privileges when possible (needed for TUN-related setup).
- Install JS deps with `pnpm install`.
- Rust toolchain is required for `backend/` (cargo, rustfmt, clippy).
- Run `pnpm check`.
- Ensure required binary resources are available before development runs.

## Common commands

- `pnpm dev:diff` runs Tauri dev + React devtools.
- `pnpm tauri:dev` runs Tauri dev only.
- `pnpm web:dev` runs the frontend app only.
- `pnpm build` builds the Tauri desktop app.
- `pnpm lint` runs eslint/prettier/stylelint/clippy.
- `pnpm fmt` runs prettier + cargo fmt.

## Code conventions

- JS/TS formatting via Prettier; lint via ESLint and Stylelint.
- Rust formatting via `cargo fmt`; lint via `cargo clippy`.
- Commit messages follow Conventional Commits (commitlint).

### code conduct

- Prefer functional style where practical; keep pure functions and side effects separate.
- Keep functions less than 50 lines when possible.
- If a feature needs lots of code, split into smaller functions; each function should have a brief comment describing execution order.

## Agent notes

- Use `pnpm` (not npm/yarn). Update `pnpm-lock.yaml` when deps change.
- Keep changes scoped; avoid touching generated files unless requested.
- Prefer workspace filters (example: `pnpm --filter=chimera-ui <script>`).
- For new features, first review `https://github.com/libnyanpasu/clash-nyanpasu` and keep code as consistent as possible.
- When debugging bugs, prioritize referencing `https://github.com/libnyanpasu/clash-nyanpasu` in local ref/
