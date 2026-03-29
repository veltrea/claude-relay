# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed
- Fix SQL LIKE injection by escaping `%` and `_` in `workspace` parameter to ensure accurate filtering (BUG-005)
- Refine VS Code client detection to avoid misidentifying unrelated processes like `recode` or `node` (BUG-006)
- Handle `id` arguments passed as float in `memory_get_entry` to prevent missing entries (BUG-007)
- Fix multibyte string slicing panic: replaced byte-based truncation with safe character boundary slicing to prevent `byte index is not a char boundary` errors during `memory_search` (BUG-008)

## [0.2.1] - 2026-03-27

### Fixed
- Fix `limit` parameter parsing: accept both JSON integer and float (AI may pass integers as floats)
- Fix `memory_unlock_cross_scope`: accept `confirmed` as both JSON boolean and string `"true"`
- Fix `db::init`: create `client` index after migration to avoid column-not-found error
- Remove emoji characters from MCP response messages
- Fix workspace scoping: entries with `cwd IS NULL` were silently excluded from workspace-scoped searches due to SQL `NULL LIKE x = NULL` semantics (BUG-009); now included via `OR cwd IS NULL`

## [0.2.0] - 2026-03-27

### Fixed
- Remove orphaned `memory_get_summary` tool that referenced a non-existent summaries table
  - The summaries feature was removed in an earlier refactor, but the MCP tool was left behind, causing AI to report errors when it tried to call it

### Added
- Workspace scoping: memory tools now default to the current workspace only, preventing unrelated workspaces from polluting the context window
- `memory_unlock_cross_scope` tool: cross-workspace search requires explicit user approval per session
- Client detection: MCP server detects the launching AI client via parent process (PPID) and MCP `initialize` `clientInfo`
- `client` field added to `raw_entries` table (auto-migrated on first run)
- Sync-on-tool-call fallback: JSONL sync runs automatically on every MCP tool call in addition to the session start hook
- GitHub Actions release workflow: multi-platform binaries (macOS arm64/x86_64, Linux x86_64, Windows x86_64)

## [0.1.0] - 2026-03-24

### Initial Release
- SQLite-backed session memory for Claude Code
- MCP server with `memory_search`, `memory_get_entry`, `memory_list_sessions`, `memory_get_session`
- JSONL incremental sync with byte-offset tracking
- Session start hook and summary hook support
- CLI commands: `serve`, `db`, `list`, `export`, `ingest`, `sync`, `query`, `write`
- Archive strategy: retention days + Markdown export
