use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use crate::config::Config;
use crate::db;
use crate::detect;
use crate::ingest;

/// サーバー状態
struct ServerState {
    workspace: Option<String>,
    client: String,
    /// セッション中の横断検索許可フラグ
    /// false = ワークスペース内に限定（デフォルト）
    /// true  = ユーザーが明示的に許可した後のみ
    cross_scope_allowed: bool,
}

/// MCP stdio サーバーを起動
pub fn serve(workspace: Option<String>) -> Result<()> {
    let db_path = Config::db_path();
    let conn = db::open(&db_path)?;
    db::init(&conn)?;

    if let Err(e) = ingest::sync_all(&conn) {
        eprintln!("claude-relay: startup sync error: {e}");
    }

    let detected_client = detect::detect_from_ppid();

    let mut state = ServerState {
        workspace: workspace.or_else(|| std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string())),
        client: detected_client,
        cross_scope_allowed: false,
    };

    eprintln!(
        "claude-relay: mcp server started. client={} workspace={}",
        state.client,
        state.workspace.as_deref().unwrap_or("(none)")
    );

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // initialize から clientInfo と roots を取得
        if request.get("method").and_then(|m| m.as_str()) == Some("initialize") {
            let params = request.get("params").cloned().unwrap_or(json!({}));

            if let Some(name) = params
                .get("clientInfo")
                .and_then(|c| c.get("name"))
                .and_then(|n| n.as_str())
            {
                let normalized = detect::normalize_client_info(name);
                eprintln!("claude-relay: client from clientInfo={normalized} (raw={name})");
                state.client = normalized;
            }

            if state.workspace.is_none() {
                if let Some(uri) = params
                    .get("roots")
                    .and_then(|r| r.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|r| r.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    let path = uri.strip_prefix("file://").unwrap_or(uri);
                    state.workspace = Some(path.to_string());
                    eprintln!("claude-relay: workspace from roots={path}");
                }
            }
        }

        let response = handle_request(&conn, &request, &mut state);
        if response.is_null() {
            continue;
        }
        let response_str = serde_json::to_string(&response)?;
        writeln!(stdout, "{response_str}")?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(conn: &rusqlite::Connection, request: &Value, state: &mut ServerState) -> Value {
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => {
            let is_claude = state.client == "claude-code";
            let mut result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "claude-relay",
                    "version": "0.2.0",
                    "detectedClient": state.client
                }
            });

            if !is_claude {
                result["_warning"] = json!(
                    "[WARNING] claude-relay is designed for Claude Code. \
                     Running under a different client may cause unexpected behavior. \
                     Full multi-client support is planned for agents-relay (next major version)."
                );
            }

            json!({ "jsonrpc": "2.0", "id": id, "result": result })
        }
        "notifications/initialized" => Value::Null,
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_definitions() }
        }),
        "tools/call" => {
            let result = handle_tool_call(conn, &params, state);
            match result {
                Ok(content) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": content }]
                    }
                }),
                Err(e) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Error: {e}") }],
                        "isError": true
                    }
                }),
            }
        }
        _ => {
            if request.get("id").is_none() {
                eprintln!("claude-relay: ignoring notification: {method}");
                return Value::Null;
            }
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method not found: {method}") }
            })
        }
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "memory_search",
            "description": "Search session memory by keyword and/or date. \
                Results are always scoped to the current workspace. \
                To search across all workspaces, first call memory_unlock_cross_scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":      { "type": "string", "description": "FTS search query" },
                    "date":       { "type": "string", "description": "Filter by date (YYYY-MM-DD)" },
                    "date_from":  { "type": "string", "description": "Date range start (YYYY-MM-DD)" },
                    "date_to":    { "type": "string", "description": "Date range end (YYYY-MM-DD)" },
                    "type":       { "type": "string", "description": "Filter by type: user / assistant / system" },
                    "session_id": { "type": "string", "description": "Limit to a specific session" },
                    "limit":      { "type": "number", "description": "Max results (default 20)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "memory_get_entry",
            "description": "Get full content of a specific memory entry by ID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "number", "description": "Entry ID from search results" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "memory_list_sessions",
            "description": "List recent sessions with timestamps and entry counts. \
                Shows only sessions from the current workspace by default. \
                To list sessions across all workspaces, first call memory_unlock_cross_scope.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "date":  { "type": "string", "description": "Filter by date (YYYY-MM-DD)" },
                    "limit": { "type": "number", "description": "Max results (default 10)" }
                }
            }
        },
        {
            "name": "memory_get_session",
            "description": "Get the conversation flow of a specific session by ID.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "type":       { "type": "string", "description": "Filter by type (default: user,assistant)" },
                    "limit":      { "type": "number", "description": "Max results (default 50)" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "memory_unlock_cross_scope",
            "description": "Request permission to search memory across ALL workspaces (not just the current one). \
                IMPORTANT: You MUST ask the user for explicit approval before calling this with confirmed=true. \
                Say something like: 「他のワークスペースの記憶も参照してよいですか？」 and wait for their answer. \
                Only set confirmed=true if the user says yes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "confirmed": {
                        "type": "boolean",
                        "description": "Set to true ONLY after the user has explicitly approved cross-workspace access."
                    }
                }
            }
        }
    ])
}

fn handle_tool_call(conn: &rusqlite::Connection, params: &Value, state: &mut ServerState) -> Result<String> {
    let tool_name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    if let Err(e) = ingest::sync_all(conn) {
        eprintln!("Sync error: {e}");
    }

    // ワークスペースフィルタ: cross_scope_allowed の時のみ None（全件）
    let ws_filter = if state.cross_scope_allowed {
        None
    } else {
        state.workspace.as_deref()
    };

    match tool_name {
        "memory_search" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let date = args.get("date").and_then(|d| d.as_str());
            let date_from = args.get("date_from").and_then(|d| d.as_str());
            let date_to = args.get("date_to").and_then(|d| d.as_str());
            let entry_type = args.get("type").and_then(|t| t.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let limit = args.get("limit")
                .and_then(|l| l.as_i64().or_else(|| l.as_f64().map(|f| f as i64)).or_else(|| l.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(20);

            let entries = db::search(conn, query, date, date_from, date_to, entry_type, session_id, limit, ws_filter)?;
            Ok(serde_json::to_string_pretty(&format_entries(&entries))?)
        }
        "memory_get_entry" => {
            let id = args.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
            match db::get_entry(conn, id)? {
                Some(e) => Ok(serde_json::to_string_pretty(&json!({
                    "id": e.id,
                    "session_id": e.session_id,
                    "timestamp": e.timestamp,
                    "date": e.date,
                    "time": e.time,
                    "type": e.entry_type,
                    "tool_name": e.tool_name,
                    "content": e.content,
                    "cwd": e.cwd,
                    "git_branch": e.git_branch,
                    "client": e.client,
                }))?),
                None => Ok(format!("No entry found with id: {id}")),
            }
        }
        "memory_list_sessions" => {
            let date = args.get("date").and_then(|d| d.as_str());
            let limit = args.get("limit")
                .and_then(|l| l.as_i64().or_else(|| l.as_f64().map(|f| f as i64)).or_else(|| l.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(10);
            let sessions = db::list_sessions(conn, date, limit, ws_filter)?;
            let result: Vec<Value> = sessions
                .iter()
                .map(|(sid, first, last, date, count)| json!({
                    "session_id": sid,
                    "first_timestamp": first,
                    "last_timestamp": last,
                    "date": date,
                    "entry_count": count,
                }))
                .collect();
            Ok(serde_json::to_string_pretty(&result)?)
        }
        "memory_get_session" => {
            let session_id = args.get("session_id").and_then(|s| s.as_str()).unwrap_or("");
            let entry_type = args.get("type").and_then(|t| t.as_str());
            let limit = args.get("limit")
                .and_then(|l| l.as_i64().or_else(|| l.as_f64().map(|f| f as i64)).or_else(|| l.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(50);
            let entries = db::get_session_entries(conn, session_id, entry_type, limit)?;
            let result: Vec<Value> = entries
                .iter()
                .map(|e| json!({
                    "id": e.id,
                    "timestamp": e.timestamp,
                    "time": e.time,
                    "type": e.entry_type,
                    "tool_name": e.tool_name,
                    "content": e.content,
                }))
                .collect();
            Ok(serde_json::to_string_pretty(&result)?)
        }
        "memory_unlock_cross_scope" => {
            let confirmed = args.get("confirmed")
                .map(|c| c.as_bool().unwrap_or_else(|| c.as_str() == Some("true")))
                .unwrap_or(false);

            if !confirmed {
                // AIへの指示: ユーザーに聞いてから confirmed=true で呼び直せ
                return Ok(json!({
                    "status": "confirmation_required",
                    "message": "Cross-workspace memory access requested.\n\
                        Please ask the user: 「他のワークスペースの記憶も参照してよいですか？」\n\
                        If the user approves, call this tool again with confirmed=true.\n\
                        If the user declines, do NOT call again — keep using workspace-scoped tools."
                }).to_string());
            }

            // ユーザーが承認 → セッション中フラグを立てる
            state.cross_scope_allowed = true;
            eprintln!("claude-relay: cross-scope access granted by user");

            Ok(json!({
                "status": "unlocked",
                "message": "Cross-workspace memory access is now enabled for this session. \
                    All memory tools will now search across all workspaces."
            }).to_string())
        }
        _ => anyhow::bail!("Unknown tool: {tool_name}"),
    }
}

fn format_entries(entries: &[db::RawEntry]) -> Vec<Value> {
    entries
        .iter()
        .map(|e| json!({
            "id": e.id,
            "session_id": e.session_id,
            "timestamp": e.timestamp,
            "date": e.date,
            "time": e.time,
            "type": e.entry_type,
            "tool_name": e.tool_name,
            "content": e.content,
            "cwd": e.cwd,
            "git_branch": e.git_branch,
            "client": e.client,
        }))
        .collect()
}
