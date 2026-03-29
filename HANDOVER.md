# claude-relay ハンドオーバー文書

**作成日:** 2026-03-29
**バージョン:** v0.2.1
**リポジトリ:** https://github.com/veltrea/claude-relay

---

## プロジェクト概要

Claude CLI の会話ログ（JSONL）を SQLite に取り込み、MCP ツール経由で過去の会話を検索・参照できるようにするメモリ拡張ツール。Rust 製シングルバイナリ、外部依存なし。

## 現在の状態

| 項目 | 状態 |
|---|---|
| コアパイプライン（ingest → DB → search） | ✅ 完成・テスト済み |
| ワークスペーススコーピング | ✅ 実装済み |
| クロスワークスペース検索（unlock_cross_scope） | ✅ 実装済み |
| クライアント検出（Claude Code, Cursor等） | ✅ 実装済み |
| GitHub Actions リリースワークフロー | ✅ macOS/Linux/Windows |
| E2E リコールテスト | ✅ 30件 100% PASS |
| レイヤー別テスト（L1〜L3） | ✅ スクリプト作成済み |

## ソースコード構成

```
src/
├── main.rs      (433行) CLI エントリポイント、サブコマンド定義
├── db.rs        (444行) SQLite 操作、マイグレーション、FTS5
├── mcp.rs       (380行) MCP サーバー、ツール定義・ハンドラ
├── ingest.rs    (316行) JSONL パーサー、差分取り込み
├── detect.rs    (104行) クライアント検出（PPID, clientInfo）
├── config.rs    (100行) 設定管理
└── export.rs    ( 66行) セッションの Markdown エクスポート
```

合計: 約1,843行

## MCP ツール一覧

| ツール名 | 機能 |
|---|---|
| `memory_search` | キーワード/日付でセッション記憶を全文検索 |
| `memory_get_entry` | ID 指定で記憶エントリの全文取得 |
| `memory_list_sessions` | セッション一覧（タイムスタンプ、件数） |
| `memory_get_session` | セッション内の会話フロー取得 |
| `memory_unlock_cross_scope` | 他ワークスペースの記憶参照を許可（要ユーザー確認） |

## 既知の注意点・ノウハウ

### AI が渡すパラメータの型問題

AIはJSONスキーマ通りの型を渡すとは限らない:

```rust
// integer → AI が float (3.0) で渡すことがある
let limit = args.get("limit")
    .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
    .unwrap_or(20) as usize;

// boolean → AI が文字列 "true" で渡すことがある
let confirmed = args.get("confirmed")
    .map(|c| c.as_bool().unwrap_or_else(|| c.as_str() == Some("true")))
    .unwrap_or(false);
```

### テスト方法

JSON-RPC 流し込みテストだけでは不十分。**必ず AI 自身に実際にツールを呼ばせて動作確認する**こと。

## テスト結果サマリ

### ワンショットモード（`claude -p`）テスト

- **方式:** `claude -p "Please acknowledge: RELAY_TEST_XXXX_..."` で30件のランダムフレーズを逐次送信
- **目的:** Claude CLI → JSOL 記録 → ingest → DB → memory_search の全パイプライン検証
- **結果:** 30/30 = **100% recall** (3.5分で完了)
- **ポイント:** `< /dev/null` で stdin 競合を回避する必要あり

### インタラクティブモードテスト

- **方式:** tmux で Claude を対話的に起動し、フレーズを送信後 `/exit` で終了
- **目的:** ワンショットではなく実際の対話セッションで JSOL が正しく記録されるか検証
- **結果:** 30/30 = **100% recall**
- **ポイント:** 起動に10秒以上かかるため、ワンショットより時間がかかる

### テストの統計的根拠

- 30件は中心極限定理の最低ライン
- ソフトウェアテストでは「全部通るか全部落ちるか」になりやすいため、30件で十分な信頼性
- 95%信頼区間で recall rate ±18% の精度

## 後継プロジェクト: agents-relay

`/Volumes/2TB_USB/dev/agents-relay` にマルチエージェント対応版を開発中。claude-relay のコアロジックをベースに拡張。

## セットアップ手順

```bash
# ビルド
cargo build --release

# Claude Code の settings.json に追加
# mcpServers → claude-relay → command: "/path/to/claude-relay", args: ["serve"]

# 手動 ingest
claude-relay ingest ~/.claude/projects

# CLI で検索テスト
claude-relay tool memory_search --query "キーワード"
```

## ファイル一覧（リポジトリ）

```
├── Cargo.toml           パッケージ定義
├── src/                 Rust ソースコード
├── docs/
│   ├── ARCHITECTURE.md  アーキテクチャ詳細
│   └── blog_testing_strategy.md
├── tests/
│   ├── memory_recall_test.sh     ワンショット E2E テスト
│   ├── memory_recall_test_v2.sh  インタラクティブ E2E テスト
│   ├── seed_worker.sh            seed ワーカー（並列版用）
│   ├── test_layer1_jsol.sh       レイヤー1 単体テスト
│   ├── test_layer2_ingest.sh     レイヤー2 単体テスト
│   ├── test_layer3_search.sh     レイヤー3 単体テスト
│   └── TEST_REPORT.md            テスト設計書・結果レポート
├── HANDOVER.md          ← この文書
└── .github/workflows/   CI/CD リリースワークフロー
```
