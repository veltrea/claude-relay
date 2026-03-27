# 新ツール設計書: local-mem 後継

## コンセプト

Claude Codeセッション間の記憶を、最小構成で実現する。
ローカルLLM不要、APIコスト不要。
Rust製シングルバイナリ。

---

## アーキテクチャ

```
┌─────────────────────────────────────────────────┐
│            保存（デーモン不要・遅延取り込み）        │
│                                                   │
│  ~/.claude/projects/**/*.jsonl                    │
│                                                   │
│  同期タイミング:                                   │
│  ① MCPツール呼び出し時（自動同期）                  │
│       ↓                                           │
│  sync_state テーブルで前回offsetを確認             │
│       ↓ 差分の新しい行だけ読む                     │
│       ↓ user/assistant type を SQLite に INSERT    │
│       ↓ 画像データ(base64)のみ除外                │
│       ↓ offset 更新                               │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│              検索（オンデマンド）                   │
│                                                   │
│  MCPツール: memory_search                           │
│       ↓ ユーザーが明示的にリクエスト               │
│       ↓ 期間指定: 「3/23の作業内容」              │
│       ↓ キーワード: 「OAuth修正の時のやつ」        │
│  SQLite FTS で生データを返す                      │
└─────────────────────────────────────────────────┘
```

---

## コンポーネント一覧

| コンポーネント | 役割 | 技術 |
|---------------|------|------|
| 遅延同期モジュール | JSONL → SQLite (差分取り込み) | Rust |
| SQLite | 生データの保管 | rusqlite |
| MCP サーバー | 記憶検索ツールを提供 (stdio) | Rust |
| CLI | DB管理・エクスポート等 | clap |

### Rust クレート依存

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
dirs = "5"
glob = "0.3"
```

---

## テーブル設計

```sql
-- 生データ（JSONL監視で自動投入）
CREATE TABLE raw_entries (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL,       -- JSONLファイル名 (UUID)
  timestamp TEXT NOT NULL,        -- 元データのタイムスタンプ (ISO 8601)
  date TEXT NOT NULL,             -- YYYY-MM-DD
  time TEXT NOT NULL,             -- HH:MM:SS
  type TEXT NOT NULL,             -- user / assistant / system 等
  tool_name TEXT,                 -- Bash, Read, Edit, etc.
  content TEXT NOT NULL,          -- メッセージ本文
  cwd TEXT,
  git_branch TEXT,
  created_at TEXT DEFAULT (datetime('now'))
);

-- FTS検索用
CREATE VIRTUAL TABLE raw_entries_fts USING fts5(
  content, tool_name, session_id
);

-- 同期状態
CREATE TABLE sync_state (
  file_path TEXT PRIMARY KEY,
  last_offset INTEGER DEFAULT 0
);
```

---

## MCP ツール定義

### 1. `memory_search`
セッション横断でキーワード・日付検索。

### 2. `memory_get_entry`
特定エントリの全文を取得。

### 3. `memory_list_sessions`
セッション一覧を取得。

### 4. `memory_get_session`
特定セッションの会話フローを取得。

---

## 設計変更履歴: summaries テーブルの削除

実運用テストの結果、`summaries` テーブル（要約の保存）を廃止しました。
- 理由: サマリーは生ログからエージェントが動的に生成する方が、文脈に適合した高精度な出力が得られるため。
- 結果: DB は「生データの高速検索エンジン」としての役割に専念。
