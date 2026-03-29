mod config;
mod db;
mod detect;
mod export;
mod ingest;
mod mcp;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "claude-relay", version, about = "Claude Code session memory")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start MCP server (stdio)
    Serve {
        /// Workspace path for scoped queries (defaults to all)
        #[arg(long)]
        workspace: Option<String>,
    },

    /// Database management
    Db {
        #[command(subcommand)]
        action: DbAction,
    },

    /// Archive old entries to Markdown
    Archive {
        /// Dry run (show what would be archived)
        #[arg(long)]
        dry: bool,
    },

    /// List sessions
    List {
        /// Filter by date (YYYY-MM-DD)
        #[arg(long)]
        date: Option<String>,
        /// Max results
        #[arg(long, default_value = "10")]
        limit: i64,
    },

    /// Export session(s) to Markdown
    Export {
        /// Session ID to export
        session_id: Option<String>,
        /// Export by date
        #[arg(long)]
        date: Option<String>,
        /// Date range start
        #[arg(long)]
        from: Option<String>,
        /// Date range end
        #[arg(long)]
        to: Option<String>,
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Write a test entry manually
    Write {
        /// Message content
        content: String,
        /// Entry type (user/assistant/system)
        #[arg(long, default_value = "user")]
        r#type: String,
        /// Session ID
        #[arg(long, default_value = "manual")]
        session: String,
    },

    /// Ingest JSONL file(s)
    Ingest {
        /// Path to JSONL file or directory
        path: PathBuf,
    },

    /// Sync state management
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },

    /// Execute raw SQL query
    Query {
        /// SQL statement
        sql: String,
    },

    /// Test MCP tools from CLI
    Tool {
        /// Tool name (memory_search, memory_list_sessions, etc.)
        name: String,
        /// Query string
        #[arg(long)]
        query: Option<String>,
        /// Date filter
        #[arg(long)]
        date: Option<String>,
        /// Session ID
        #[arg(long)]
        session_id: Option<String>,
        /// Entry ID
        #[arg(long)]
        id: Option<i64>,
        /// Max results
        #[arg(long)]
        limit: Option<i64>,
    },
}

#[derive(Subcommand)]
enum DbAction {
    /// Reset database (delete and recreate)
    Reset,
    /// Show database statistics
    Stats,
    /// Run VACUUM
    Vacuum,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,
    /// Set a configuration value
    Set {
        /// Config key
        key: String,
        /// Config value
        value: String,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// Show sync status
    Status,
    /// Reset all sync offsets
    Reset,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = config::Config::db_path();
    let mut conn = db::open(&db_path)?;
    db::init(&conn)?;

    match cli.command {
        Commands::Serve { workspace } => {
            mcp::serve(workspace)?;
        }

        Commands::Db { action } => match action {
            DbAction::Reset => {
                db::reset(&conn)?;
                println!("Database reset complete.");
            }
            DbAction::Stats => {
                println!("{}", db::stats(&conn)?);
            }
            DbAction::Vacuum => {
                conn.execute_batch("VACUUM")?;
                println!("VACUUM complete.");
            }
        },

        Commands::Archive { dry } => {
            let cfg = config::Config::load()?;
            let cutoff = chrono::Utc::now()
                - chrono::Duration::days(cfg.retention_days as i64);
            let cutoff_date = cutoff.format("%Y-%m-%d").to_string();

            let dates: Vec<String> = {
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT date FROM raw_entries WHERE date < ?1 ORDER BY date",
                )?;
                let res: Vec<String> = stmt
                    .query_map(rusqlite::params![cutoff_date], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                res
            };

            if dates.is_empty() {
                println!("No entries older than {cutoff_date} to archive.");
                return Ok(());
            }

            let archive_dir = config::resolve_archive_dir(&cfg);

            for date in &dates {
                let parts: Vec<&str> = date.split('-').collect();
                if parts.len() != 3 {
                    continue;
                }
                let dir = archive_dir.join(parts[0]).join(parts[1]);
                let file = dir.join(format!("{}.md", parts[2]));

                if dry {
                    println!("[dry] Would archive {date} -> {}", file.display());
                } else {
                    let md = export::export_date(&conn, date)?;
                    std::fs::create_dir_all(&dir)?;

                    if !file.exists() {
                        std::fs::write(&file, &md)?;
                    }

                    let tx = conn.transaction()?;
                    tx.execute(
                        "DELETE FROM raw_entries WHERE date = ?1",
                        rusqlite::params![date],
                    )?;
                    tx.commit()?;

                    println!("Archived {date} -> {}", file.display());
                }
            }

            if !dry {
                conn.execute_batch(
                    "DELETE FROM raw_entries_fts;
                     INSERT INTO raw_entries_fts (rowid, content, tool_name, session_id)
                     SELECT id, content, tool_name, session_id FROM raw_entries;",
                )?;
                println!("FTS index rebuilt.");
            }
        }

        Commands::List { date, limit } => {
            let sessions = db::list_sessions(&conn, date.as_deref(), limit, None)?;
            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!(
                    "{:<40} {:<12} {:<12} {:>6}",
                    "SESSION_ID", "DATE", "LAST_TIME", "ENTRIES"
                );
                println!("{}", "-".repeat(74));
                for (sid, _first, last, date, count) in &sessions {
                    let time_part = if last.len() >= 19 { &last[11..19] } else { last };
                    println!("{:<40} {:<12} {:<12} {:>6}", sid, date, time_part, count);
                }
            }
        }

        Commands::Export {
            session_id,
            date,
            from,
            to,
            output,
        } => {
            let md = if let Some(sid) = session_id {
                export::export_session(&conn, &sid)?
            } else if let Some(d) = date {
                export::export_date(&conn, &d)?
            } else if from.is_some() || to.is_some() {
                let f = from.as_deref().unwrap_or("2000-01-01");
                let t = to.as_deref().unwrap_or("2099-12-31");
                let mut stmt = conn.prepare(
                    "SELECT DISTINCT date FROM raw_entries WHERE date >= ?1 AND date <= ?2 ORDER BY date",
                )?;
                let dates: Vec<String> = stmt
                    .query_map(rusqlite::params![f, t], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();
                let mut combined = String::new();
                for d in &dates {
                    combined.push_str(&export::export_date(&conn, d)?);
                    combined.push('\n');
                }
                combined
            } else {
                anyhow::bail!("Specify session_id, --date, or --from/--to");
            };

            if let Some(out_path) = output {
                std::fs::create_dir_all(&out_path)?;
                let filename = out_path.join("export.md");
                std::fs::write(&filename, &md)?;
                println!("Exported to {}", filename.display());
            } else {
                print!("{md}");
            }
        }

        Commands::Config { action } => match action {
            ConfigAction::Show => {
                let cfg = config::Config::load()?;
                println!("{}", cfg.show());
            }
            ConfigAction::Set { key, value } => {
                let mut cfg = config::Config::load()?;
                cfg.set(&key, &value)?;
                println!("Set {key} = {value}");
            }
        },

        Commands::Write {
            content,
            r#type,
            session,
        } => {
            let now = chrono::Utc::now();
            let ts = now.to_rfc3339();
            let date = now.format("%Y-%m-%d").to_string();
            let time = now.format("%H:%M:%S").to_string();
            let tx = conn.transaction()?;
            let id = db::insert_entry(
                &tx, &session, &ts, &date, &time, &r#type, None, &content, None, None, "claude-code",
            )?;
            tx.commit()?;
            println!("Inserted entry id={id} type={} session={session}", r#type);
        }

        Commands::Ingest { path } => {
            let count = if path.is_dir() {
                ingest::ingest_dir(&mut conn, &path)?
            } else {
                ingest::ingest_file(&mut conn, &path)?
            };
            println!("Ingested {count} entries.");
        }

        Commands::Sync { action } => match action {
            SyncAction::Status => {
                println!("{}", ingest::sync_status(&conn)?);
            }
            SyncAction::Reset => {
                ingest::sync_reset(&conn)?;
                println!("Sync offsets reset.");
            }
        },

        Commands::Query { sql } => {
            let mut stmt = conn.prepare(&sql)?;
            let col_count = stmt.column_count();
            let col_names: Vec<String> = (0..col_count)
                .map(|i| stmt.column_name(i).unwrap_or("?").to_string())
                .collect();

            println!("{}", col_names.join("\t"));
            println!("{}", "-".repeat(col_names.len() * 16));

            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let vals: Vec<String> = (0..col_count)
                    .map(|i| {
                        row.get::<_, String>(i)
                            .or_else(|_| row.get::<_, i64>(i).map(|v| v.to_string()))
                            .or_else(|_| row.get::<_, f64>(i).map(|v| v.to_string()))
                            .unwrap_or_else(|_| "NULL".to_string())
                    })
                    .collect();
                println!("{}", vals.join("\t"));
            }
        }

        Commands::Tool {
            name,
            query,
            date,
            session_id,
            id,
            limit,
        } => {
            let sync_count = ingest::sync_all(&mut conn)?;
            if sync_count > 0 {
                eprintln!("Synced {sync_count} entries before tool call.");
            }

            match name.as_str() {
                "memory_search" => {
                    let q = query.as_deref().unwrap_or("");
                    let entries = db::search(
                        &conn,
                        q,
                        date.as_deref(),
                        None,
                        None,
                        None,
                        session_id.as_deref(),
                        limit.unwrap_or(20),
                        None, // workspace
                    )?;
                    for e in &entries {
                        let preview: String = e.content.chars().take(60).collect();
                        println!("[{}] {} | {} | {}: {}", e.id, e.date, e.time, e.entry_type, preview);
                    }
                    println!("\n{} results.", entries.len());
                }
                "memory_list_sessions" => {
                    let sessions =
                        db::list_sessions(&conn, date.as_deref(), limit.unwrap_or(10), None)?;
                    for (sid, _first, _last, date, count) in &sessions {
                        println!("{sid}  {date}  {count} entries");
                    }
                }
                "memory_get_entry" => {
                    let entry_id = id.unwrap_or(0);
                    match db::get_entry(&conn, entry_id)? {
                        Some(e) => {
                            println!(
                                "id={} session={} {} {} type={}\n{}",
                                e.id, e.session_id, e.date, e.time, e.entry_type, e.content
                            );
                        }
                        None => println!("Not found."),
                    }
                }
                "memory_get_session" => {
                    let sid = session_id.as_deref().unwrap_or("");
                    let entries =
                        db::get_session_entries(&conn, sid, None, limit.unwrap_or(50))?;
                    for e in &entries {
                        println!("[{}] {} {}: {}", e.id, e.time, e.entry_type, e.content);
                    }
                }
                _ => {
                    println!("Unknown tool: {name}");
                    println!("Available: memory_search, memory_list_sessions, memory_get_entry, memory_get_session");
                }
            }
        }
    }

    Ok(())
}
