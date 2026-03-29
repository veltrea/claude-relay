# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed
- **BUG-001/002**: Introduce batch transaction architecture in `ingest_file` — inserts to `raw_entries` and `raw_entries_fts` are now committed atomically every 100 entries together with the sync offset, preventing duplicate entries on crash recovery and eliminating FTS/raw_entries desync
- **BUG-003**: Fix byte offset drift on Windows CRLF files — replaced `lines()` iterator (which strips `\r`) with `read_line()` to count exact bytes read, ensuring accurate incremental sync across platforms
- **BUG-004**: Wrap archive DELETE in a transaction and add `file.exists()` idempotency check — prevents data loss if process is killed between file write and DB delete; also wraps `write` command insert in a transaction
- **BUG-005**: Fix SQL LIKE injection by escaping `%`, `_` and `\` in `workspace` parameter in both `search` and `list_sessions` functions
- **BUG-006**: Refine VS Code client detection — replaced broad `contains("code")` with exact match `n == "code"` and prefix checks; added unit tests for false-positive cases (`recode`, `xcode`, `node`, `electron`)
- **BUG-007**: Handle `id` arguments passed as float in `memory_get_entry` — applies same `as_i64().or_else(as_f64())` pattern already used for `limit`
- **BUG-008**: Fix multibyte string slicing panic in `memory_search` CLI output — replaced byte-based `[..120]` slice with `chars().take(60).collect()`
- **BUG-009**: Include `cwd IS NULL` entries in workspace-scoped search — `NULL LIKE x` evaluates to NULL in SQL, silently excluding entries without a recorded cwd; fixed in both `search` and `list_sessions`

## [0.2.1] - 2026-03-27

### Fixed
- Fix `limit` parameter parsing: accept both JSON integer and float (AI may pass integers as floats)
- Fix `memory_unlock_cross_scope`: accept `confirmed` as both JSON boolean and string `"true"`
- Fix `db::init`: create `client` index after migration to avoid column-not-found error
- Remove emoji characters from MCP response messages

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
