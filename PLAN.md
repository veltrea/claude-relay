# 新ツール設計書: local-mem 後継

## コンセプト

Claude Codeセッション間の記憶を、最小構成で実現する。
ローカルLLM不要、APIコスト不要、Express不要。
Rust製シングルバイナリ。

---

## アーキテクチャ

```
┌─────────────────────────────────────────────────┐
│            保存（デーモン不要・遅延取り込み）        │
│                                                   │
│  ~/.claude/projects/**/*.jsonl                    │
│                                                   │
│  同期タイミング（2箇所、どちらが走っても正しい）:    │
│  ① SessionStartフック（毎セッション開始時）         │
│  ② MCPツール呼び出し時（フォールバック）            │
│       ↓                                           │
│  sync_state テーブルで前回offsetを確認             │
│       ↓ 差分の新しい行だけ読む                     │
│       ↓ 全type を SQLite に INSERT               │
│       ↓ 画像データ(base64)のみ除外                │
│       ↓ offset 更新                               │
│  読み出し時に WHERE type で絞る                    │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│             要約（セッション終了時）                │
│                                                   │
│  Summaryフック → Claude自身がハンドオーバー生成    │
│       ↓                                           │
│  SQLite summaries テーブルに保存                   │
│  + handover.md に書き出し                         │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│              注入（セッション開始時）               │
│                                                   │
│  SessionStartフック                               │
│       ↓ SQLite summaries から直近N件取得          │
│       ↓ stdout に出力（Claude のコンテキストへ）   │
│  ※ 画像なし、要約のみ、軽量                       │
└─────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────┐
│              検索（オンデマンド）                   │
│                                                   │
│  MCPツール: search / search_raw                   │
│       ↓ ユーザーが明示的にリクエスト               │
│       ↓ 期間指定: 「3/23の作業内容」              │
│       ↓ キーワード: 「OAuth修正の時のやつ」        │
│  SQLite FTS で生データを返す                      │
│  ※ 画像はユーザーが求めた時だけ                    │
└─────────────────────────────────────────────────┘
```

---

## コンポーネント一覧

| コンポーネント | 役割 | 技術 |
|---------------|------|------|
| 遅延同期モジュール | JSONL → SQLite (差分取り込み) | Rust |
| SQLite | 生データ + 要約の保管 | rusqlite |
| MCP サーバー | 記憶検索ツールを提供 (stdio) | rmcp / Rust MCP SDK |
| CLI | DB管理・エクスポート等 | clap |
| JSONL パース | セッションログ読み込み | serde_json |
| Summaryフック | セッション終了時にClaude自身が要約生成 | Claude Code hook |
| SessionStartフック | 直近要約をコンテキストに注入 | Claude Code hook |

### Rust クレート依存

```toml
[dependencies]
rusqlite = { version = "0.31", features = ["bundled"] }  # SQLite (libsqlite3同梱)
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive"] }           # CLI引数パーサー
rmcp = { version = "0.1", features = ["server"] }         # MCP SDK
tokio = { version = "1", features = ["full"] }
dirs = "5"                                                 # ~/.claude-relay 等のパス解決
glob = "0.3"                                               # JSONL ファイル検索
```

### ビルド成果物

```
claude-relay          # シングルバイナリ（MCP + CLI 全部入り）
```

`claude-relay serve` → MCP サーバー (stdio) として起動
それ以外のサブコマンド → CLI 操作

---

## テーブル設計

```sql
-- 生データ（JSONL監視で自動投入、全type保存）
CREATE TABLE raw_entries (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL,       -- JSONLファイル名 (UUID)
  timestamp TEXT NOT NULL,        -- 元データのタイムスタンプ (ISO 8601)
  date TEXT NOT NULL,             -- YYYY-MM-DD（日付フィルタ用）
  time TEXT NOT NULL,             -- HH:MM:SS（時刻フィルタ用）
  type TEXT NOT NULL,             -- user / assistant / system / progress / queue-operation
  tool_name TEXT,                 -- Bash, Read, Edit, etc. (nullable)
  content TEXT NOT NULL,          -- メッセージ本文 or ツール入出力
  cwd TEXT,
  git_branch TEXT,
  created_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX idx_raw_session ON raw_entries(session_id);
CREATE INDEX idx_raw_timestamp ON raw_entries(timestamp);
CREATE INDEX idx_raw_date ON raw_entries(date);
CREATE INDEX idx_raw_type ON raw_entries(type);

-- FTS検索用
CREATE VIRTUAL TABLE raw_entries_fts USING fts5(
  content, tool_name, session_id
);

-- 同期状態（JSONLファイルごとの読み込み位置）
CREATE TABLE sync_state (
  file_path TEXT PRIMARY KEY,
  last_offset INTEGER DEFAULT 0   -- 前回読んだバイト位置
);

-- セッション要約（Summaryフックで生成）
CREATE TABLE summaries (
  id INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL UNIQUE,
  project_path TEXT,
  git_branch TEXT,
  started_at TEXT,
  ended_at TEXT,
  summary_md TEXT NOT NULL,       -- Claude自身が書いたハンドオーバー
  created_at TEXT DEFAULT (datetime('now'))
);
```

---

## フィルタルール

**書き込み時（JSONL → SQLite）:**
- 全 type を INSERT（user, assistant, system, progress, queue-operation）
- 画像データ（base64）のみ除外
- ツール出力の巨大なレスポンスは閾値超えで切り捨て

**読み出し時（用途に応じて WHERE で絞る）:**
- セッション開始注入: `WHERE type IN ('user', 'assistant')`
- MCP検索: ユーザーの指定に応じて全 type or 特定 type
- デバッグ: progress, system 等も参照可能

---

## MCP ツール定義

この MCP サーバーが Claude Code に提供するツール一覧。

### 1. `memory_search`

セッション横断でキーワード・日付検索。一番よく使うツール。

```
パラメータ:
  query: string (必須)       — FTS検索クエリ ("OAuth 修正" など)
  date: string (任意)        — YYYY-MM-DD で日付絞り込み ("2026-03-23")
  date_from: string (任意)   — 期間指定の開始日
  date_to: string (任意)     — 期間指定の終了日
  type: string (任意)        — user / assistant / system 等で絞り込み
  session_id: string (任意)  — 特定セッションに限定
  limit: number (任意)       — 返す件数 (デフォルト 20)

戻り値:
  [ { id, session_id, timestamp, date, time, type, tool_name, content(抜粋), cwd, git_branch } ]
```

### 2. `memory_get_entry`

search で見つけた特定エントリの全文を取得。

```
パラメータ:
  id: number (必須)          — raw_entries の id

戻り値:
  { id, session_id, timestamp, date, time, type, tool_name, content(全文), cwd, git_branch }
```

### 3. `memory_list_sessions`

セッション一覧を取得。「最近どんな作業してた？」に答える。

```
パラメータ:
  date: string (任意)        — 特定日のセッションだけ
  limit: number (任意)       — 返す件数 (デフォルト 10)

戻り値:
  [ { session_id, first_timestamp, last_timestamp, date, entry_count, summary(あれば) } ]
```

### 4. `memory_get_session`

特定セッションの会話フローを取得。

```
パラメータ:
  session_id: string (必須)
  type: string (任意)        — user / assistant 等で絞り込み (デフォルト: user,assistant)
  limit: number (任意)       — 返す件数 (デフォルト 50)

戻り値:
  [ { id, timestamp, time, type, tool_name, content(抜粋) } ]
```

### 5. `memory_get_summary`

セッション要約を取得。SessionStart フックでも使う。

```
パラメータ:
  session_id: string (任意)  — 指定なしなら直近N件
  limit: number (任意)       — 返す件数 (デフォルト 5)

戻り値:
  [ { session_id, project_path, git_branch, started_at, ended_at, summary_md } ]
```

---

## CLI コマンド（人間が直接叩く管理用）

MCPツールはAI経由でトークンを消費するため、管理操作はCLIで提供する。

```bash
# MCP サーバー
claude-relay serve          # stdio MCP サーバーとして起動（Claude Codeから呼ばれる）

# データベース管理
claude-relay db reset       # DB全削除して再作成
claude-relay db stats       # エントリ数・サイズ・日付範囲等
claude-relay db vacuum      # SQLite VACUUM 実行

# アーカイブ
claude-relay archive        # retention_days超えをMDに書き出してDB削除
claude-relay archive --dry  # 削除対象を表示するだけ（実行しない）

# 閲覧・書き出し
claude-relay list                        # セッション一覧（日時・エントリ数・要約の冒頭）
claude-relay list --date 2026-03-23      # 特定日のセッションだけ
claude-relay list --limit 20             # 表示件数指定

claude-relay export <session_id>         # 特定セッションをMDで標準出力
claude-relay export --date 2026-03-23    # その日の全セッションをMD書き出し
claude-relay export --date 2026-03-23 -o ~/exports/  # ファイルに書き出し
claude-relay export --from 2026-03-20 --to 2026-03-24  # 期間指定

# 設定
claude-relay config show    # 現在の設定を表示
claude-relay config set retention_days 90
claude-relay config set archive_dir ~/my-archive
```

# テスト・動作確認
claude-relay write "テストメッセージ" --type user              # 手動で記憶書き込み
claude-relay write "応答テスト" --type assistant --session test-001

claude-relay ingest <path/to/file.jsonl>           # 特定JSONLを即取り込み
claude-relay ingest ~/.claude/projects/             # ディレクトリ配下を全部取り込み

claude-relay sync status                           # 各JSONLの取り込み状況（offset, 未処理行数）
claude-relay sync reset                            # offset全リセット（再取り込み用）

claude-relay query "SELECT * FROM raw_entries ORDER BY id DESC LIMIT 5"  # 生SQL実行

claude-relay tool memory_search --query "OAuth" --date 2026-03-23  # MCPツールをCLIから直接テスト
claude-relay tool memory_list_sessions --limit 5

※ コマンド名 `claude-relay` は仮。`cargo install` or PATH に配置。

---

## local-mem からの削除対象

| 削除するもの | 理由 |
|-------------|------|
| Express APIサーバー (port 37777) | MCP直接で十分 |
| CompressionWorker | ローカルLLM圧縮は廃止 |
| LM Studio プロバイダー | 不要 |
| Chroma (ベクトル検索) | FTSで十分 |
| PostToolUseフック | JSONL監視で代替 |
| UserPromptSubmitフック | JSONL監視で代替 |
| SessionEndフック | Summaryフックに統合 |
| React製ビューアーUI | 後で必要なら別途 |
| pending_messages テーブル | 不要 |
| observations テーブル | raw_entries + summaries に置換 |

---

## 残すフック（2つだけ）

1. **SessionStart** — JSONL差分取り込み → summariesテーブルから直近N件を読んでstdoutに出力
2. **Summary** — Claude自身がセッション要約を生成、SQLiteに保存

---

## アーカイブ戦略

- SQLite には直近 `retention_days` 分のみ保持
- 期限切れデータは Markdown ファイルに書き出してから DB から削除
- アーカイブは日単位で `archive_dir/YYYY/MM/DD.md` に出力
- Markdown なので人間が直接読める・grep できる

---

## 設定ファイル

```jsonc
// ~/.claude-relay/config.json
{
  "retention_days": 30,                        // SQLiteに残す日数（デフォルト30）
  "archive_dir": "~/.claude-relay/archive"     // MDアーカイブ出力先
}
```

- ユーザーが自由に日数を調整可能
- ディスクに余裕があれば 90 日、絞りたければ 7 日など

---

## 想定コスト

- API: ¥0（ローカルLLM不要、クラウドAPI不要）
- ストレージ: SQLiteのみ（数百MB程度）
- CPU: デーモン不要（フック・ツール呼び出し時のみ動作）
