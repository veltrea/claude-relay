# claude-relay ハンドオーバー文書

作成日: 2026-03-30（更新）

---

## セッション引き継ぎの注意

このセッションは **`agents-relay` ワークスペース**で作業していたにもかかわらず、
`claude-relay` のコードを診断・修正していた。
ワークスペースの取り違えと、メモリスコーピングが効いていなかったことが原因。

**次のセッションでは必ず `/Volumes/2TB_USB/dev/claude-relay/` を正しいワークスペースとして開くこと。**
**`claude-relay serve --workspace /Volumes/2TB_USB/dev/claude-relay` でスコープを効かせること。**

---

## プロジェクト概要

Claude Code のセッション記憶をSQLiteに蓄積し、MCPツール経由でAIが過去の会話を検索できるようにするツール。

- **バイナリ**: `target/release/claude-relay`
- **DB**: `~/.claude-relay/memory.db`
- **MCP起動**: `claude-relay serve --workspace <path>`
- **GitHub**: https://github.com/veltrea/claude-relay

---

## このセッションでやったこと

### 1. GitHub Actionsの拡張（commit: `0fad0ad`）
5プラットフォーム → 11プラットフォームに拡張。

| OS | アーキテクチャ |
|---|---|
| macOS | arm64, x86_64 |
| Linux (musl static) | x86_64, arm64, armv7, i686 |
| Windows (MSVC) | x86_64, arm64, i686 |
| FreeBSD | x86_64 |

### 2. バグ分析と修正（commit: `f239f84`, `7f371bb`）

全9件のバグを発見・修正・ドキュメント化した。詳細は `docs/bugs/` を参照。

| BUG | 重要度 | 内容 | 状態 |
|---|---|---|---|
| 001 | 🔴 Critical | クラッシュ時の重複挿入 | ✅ 修正済み |
| 002 | 🔴 Critical | FTS/raw_entriesのトランザクション欠如 | ✅ 修正済み |
| 003 | 🟠 Major | Windowsオフセットズレ（CRLF） | ✅ 修正済み |
| 004 | 🟠 Major | Archiveコマンドのトランザクション欠如 | ✅ 修正済み |
| 005 | 🟠 Major | workspaceフィルタのLIKE特殊文字 | ✅ 修正済み |
| 006 | 🟡 Minor | detect.rsの"code"マッチが広すぎる | ✅ 修正済み |
| 007 | 🟡 Minor | id引数のfloat非対応（MCP経由） | ✅ 修正済み |
| 008 | 🟡 Minor | マルチバイト文字でパニック可能性 | ✅ 修正済み |
| 009 | 🟡 Minor | NULL cwdがワークスペーススコープで除外 | ✅ 修正済み |

修正はAntigravity（Sonnet 4.6）が `docs/bugs/` の指示書を読んで実施。
Claude（このセッション）がコードレビューとビルド・テスト確認を担当。

### 3. リグレッションテスト作成（commit: `4a3229c`）

`tests/bug_regression_test.sh` — BUG-001〜009の修正を自動検証するスクリプト。

```bash
bash tests/bug_regression_test.sh
# → 18 PASS, 0 FAIL, 1 SKIP (BUG-007はMCP経由のみ)
```

- テスト用DBを `HOME` 上書きで本番から完全隔離
- BUG-007のみSKIP（CLIでは再現不可、Claudeに実際に呼ばせて確認が必要）

---

## 残タスク

### 優先度高

- [ ] **バージョンタグを打つ** — 全バグ修正済みのため `v0.3.0` リリース推奨
  ```bash
  git tag v0.3.0 && git push origin v0.3.0
  ```
  → GitHub Actionsが11プラットフォーム向けバイナリをビルドしてリリースに添付する

- [ ] **BUG-007の実機テスト** — Claudeに `memory_get_entry` をid=整数で呼ばせて、
  floatで渡されても正しく動作することを確認する

### 優先度中

- [ ] **ワークスペーススコープを有効にして運用する**
  スコープなしだと全ワークスペースの記憶が混線する（今回実際に体験した問題）。

- [ ] **agents-relayへの機能マージ検討** — agents-relayはclaude-relayの後継。
  今回の修正（BUG-001〜009）をagents-relay側にも適用すべきか検討する。

---

## ビルド・テスト手順

```bash
cd /Volumes/2TB_USB/dev/claude-relay

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"
cargo build --release   # ビルド
cargo test              # 単体テスト
bash tests/bug_regression_test.sh  # リグレッションテスト（全BUG検証）
```

---

## ファイル構成

```
claude-relay/
├── src/
│   ├── main.rs       # CLIエントリポイント
│   ├── db.rs         # SQLite操作（FTS5, search, ingest）
│   ├── ingest.rs     # JSOLファイル取り込み（バッチトランザクション）
│   ├── mcp.rs        # MCPサーバー（memory_search等のツール定義）
│   ├── detect.rs     # クライアント検出（PPID/clientInfo）
│   ├── config.rs     # 設定管理
│   └── export.rs     # Markdownエクスポート
├── docs/
│   ├── bugs/         # BUG-001〜009の詳細ドキュメント（全修正済み）
│   └── antigravity_compatibility.md
├── tests/
│   ├── bug_regression_test.sh   # バグリグレッションテスト
│   └── memory_recall_test.sh    # E2Eリコールテスト（claude -p経由）
├── .github/workflows/release.yml  # 11プラットフォームCI/CD
├── changelog.md
└── HANDOVER.md  ← このファイル
```
