# Repository Guidelines

## Project Structure & Module Organization

TimeGenie is a Tauri 2 desktop app with a Vue 3 frontend and Rust backend. Frontend code lives in `src-ui/src`: views in `views/`, reusable UI in `components/`, shared state in `store.ts`, services in `services/`, global styles in `styles.css`, and media in `assets/`. Backend commands and local-first data logic live in `src-tauri/src`; SQLite migrations are in `src-tauri/migrations`. Supabase schema and integration SQL are in `supabase/`. Docs are in `docs/`, release helpers in `scripts/release`, verification scripts in `scripts/`, and OpenSpec artifacts in `openspec/`.

## Build, Test, and Development Commands

- `npm install`: install Node dependencies.
- `npm run tauri:dev`: run the desktop app in development mode.
- `npm run dev`: run only the Vite frontend on `127.0.0.1`.
- `npm run typecheck`: run Vue/TypeScript type checks.
- `npm run build`: type-check and build the frontend.
- `npm run tauri:build`: create the packaged desktop app.
- `npm run docs:dev` / `npm run docs:build`: run or build the docs site.

## Coding Style & Naming Conventions

Use TypeScript for frontend logic and Rust 2021 for backend logic. Follow the existing Vue style: `<script setup lang="ts">`, two-space indentation, double quotes, semicolons, and PascalCase components such as `TaskTable.vue`. Name services with clear camelCase names, for example `completionFeedbackCore.ts`. Keep Rust modules snake_case and domain-based, such as `time_tracking.rs`. Run `npm run typecheck` for TypeScript changes; use `cargo fmt --manifest-path src-tauri/Cargo.toml` when Rust files change.

## Testing Guidelines

Use focused verification scripts for frontend business logic: `npm run test:allocation-math` and `npm run test:completion-feedback`. Rust tests run through Cargo, for example `cargo test --manifest-path src-tauri/Cargo.toml`. The Supabase two-device test requires environment variables and a disposable account; run `npm run test:supabase:e2e` only against a prepared local or test project.

## Commit & Pull Request Guidelines

Recent history uses short, direct subjects, often Chinese, with release commits like `release: v0.2.0`. Keep titles concise and outcome-focused. For pull requests, include the user-visible change, touched areas (`src-ui`, `src-tauri`, `supabase`, etc.), verification commands, and screenshots or short recordings for UI changes. Link the relevant issue or OpenSpec change when one exists.

## Security & Configuration Tips

Do not commit real Supabase keys, SeaTable tokens, credential-store data, generated local databases, or `target/` and `dist/` artifacts. Use `supabase/schema.sql` as the source of truth for cloud schema changes and add matching local migrations under `src-tauri/migrations` when local SQLite data changes.
