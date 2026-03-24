use anyhow::Result;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use crate::config::Config;
use crate::db;
use crate::ingest;

/// MCP stdio サーバーを起動
pub fn serve() -> Result<()> {
    let db_path = Config::db_path();
    let conn = db::open(&db_path)?;
    db::init(&conn)?;

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

        let response = handle_request(&conn, &request);
        let response_str = serde_json::to_string(&response)?;
        writeln!(stdout, "{response_str}")?;
        stdout.flush()?;
    }

    Ok(())
}

fn handle_request(conn: &rusqlite::Connection, request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or(json!({}));

    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "claude-relay",
                    "version": "0.1.0"
                }
            }
        }),
        "notifications/initialized" => {
            // 通知なので返答不要だが、念のため
            return Value::Null;
        }
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": tool_definitions()
            }
        }),
        "tools/call" => {
            let result = handle_tool_call(conn, &params);
            match result {
                Ok(content) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": content
                        }]
                    }
                }),
                Err(e) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("Error: {e}")
                        }],
                        "isError": true
                    }
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": format!("Method not found: {method}")
            }
        }),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "memory_search",
            "description": "Search session memory by keyword and date. Returns matching entries across all sessions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "FTS search query" },
                    "date": { "type": "string", "description": "Filter by date (YYYY-MM-DD)" },
                    "date_from": { "type": "string", "description": "Date range start" },
                    "date_to": { "type": "string", "description": "Date range end" },
                    "type": { "type": "string", "description": "Filter by type (user/assistant/system)" },
                    "session_id": { "type": "string", "description": "Filter by session" },
                    "limit": { "type": "number", "description": "Max results (default 20)" }
                },
                "required": ["query"]
            }
        },
        {
            "name": "memory_get_entry",
            "description": "Get full content of a specific entry by ID.",
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
            "description": "List recent sessions with timestamps and entry counts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "date": { "type": "string", "description": "Filter by date (YYYY-MM-DD)" },
                    "limit": { "type": "number", "description": "Max results (default 10)" }
                }
            }
        },
        {
            "name": "memory_get_session",
            "description": "Get conversation flow of a specific session.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" },
                    "type": { "type": "string", "description": "Filter by type (default: user,assistant)" },
                    "limit": { "type": "number", "description": "Max results (default 50)" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "memory_get_summary",
            "description": "Get session summaries (handover notes).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Specific session (omit for recent)" },
                    "limit": { "type": "number", "description": "Max results (default 5)" }
                }
            }
        }
    ])
}

fn handle_tool_call(conn: &rusqlite::Connection, params: &Value) -> Result<String> {
    let tool_name = params
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(json!({}));

    // ツール呼び出し前にフォールバック同期
    if let Err(e) = ingest::sync_all(conn) {
        eprintln!("Sync warning: {e}");
    }

    match tool_name {
        "memory_search" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let date = args.get("date").and_then(|d| d.as_str());
            let date_from = args.get("date_from").and_then(|d| d.as_str());
            let date_to = args.get("date_to").and_then(|d| d.as_str());
            let entry_type = args.get("type").and_then(|t| t.as_str());
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let limit = args.get("limit").and_then(|l| l.as_i64()).unwrap_or(20);

            let entries = db::search(conn, query, date, date_from, date_to, entry_type, session_id, limit)?;
            let result: Vec<Value> = entries
                .iter()
                .map(|e| {
                    json!({
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
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&result)?)
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
                }))?),
                None => Ok(format!("No entry found with id: {id}")),
            }
        }
        "memory_list_sessions" => {
            let date = args.get("date").and_then(|d| d.as_str());
            let limit = args.get("limit").and_then(|l| l.as_i64()).unwrap_or(10);
            let sessions = db::list_sessions(conn, date, limit)?;
            let result: Vec<Value> = sessions
                .iter()
                .map(|(sid, first, last, date, count)| {
                    json!({
                        "session_id": sid,
                        "first_timestamp": first,
                        "last_timestamp": last,
                        "date": date,
                        "entry_count": count,
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&result)?)
        }
        "memory_get_session" => {
            let session_id = args
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let entry_type = args.get("type").and_then(|t| t.as_str());
            let limit = args.get("limit").and_then(|l| l.as_i64()).unwrap_or(50);
            let entries = db::get_session_entries(conn, session_id, entry_type, limit)?;
            let result: Vec<Value> = entries
                .iter()
                .map(|e| {
                    json!({
                        "id": e.id,
                        "timestamp": e.timestamp,
                        "time": e.time,
                        "type": e.entry_type,
                        "tool_name": e.tool_name,
                        "content": e.content,
                    })
                })
                .collect();
            Ok(serde_json::to_string_pretty(&result)?)
        }
        "memory_get_summary" => {
            let session_id = args.get("session_id").and_then(|s| s.as_str());
            let limit = args.get("limit").and_then(|l| l.as_i64()).unwrap_or(5);
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                if let Some(sid) = session_id {
                    (
                        "SELECT session_id, project_path, git_branch, started_at, ended_at, summary_md
                         FROM summaries WHERE session_id = ?1 LIMIT ?2".to_string(),
                        vec![Box::new(sid.to_string()), Box::new(limit)],
                    )
                } else {
                    (
                        "SELECT session_id, project_path, git_branch, started_at, ended_at, summary_md
                         FROM summaries ORDER BY created_at DESC LIMIT ?1".to_string(),
                        vec![Box::new(limit)],
                    )
                };
            let params_ref: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_ref.as_slice(), |row| {
                Ok(json!({
                    "session_id": row.get::<_, String>(0)?,
                    "project_path": row.get::<_, Option<String>>(1)?,
                    "git_branch": row.get::<_, Option<String>>(2)?,
                    "started_at": row.get::<_, Option<String>>(3)?,
                    "ended_at": row.get::<_, Option<String>>(4)?,
                    "summary_md": row.get::<_, String>(5)?,
                }))
            })?;
            let result: Vec<Value> = rows.filter_map(|r| r.ok()).collect();
            Ok(serde_json::to_string_pretty(&result)?)
        }
        _ => anyhow::bail!("Unknown tool: {tool_name}"),
    }
}
