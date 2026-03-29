# claude-relay バグトラッカー

分析日: 2026-03-30

## 一覧

| # | ファイル | 重要度 | タイトル | ステータス |
|---|---|---|---|---|
| [BUG-001](./BUG-001-duplicate-ingest.md) | ingest.rs | 🔴 Critical | クラッシュ時の重複挿入 | 未対応 |
| [BUG-002](./BUG-002-fts-transaction.md) | db.rs | 🔴 Critical | FTS/raw_entriesのトランザクション欠如 | 未対応 |
| [BUG-003](./BUG-003-windows-offset.md) | ingest.rs | 🟠 Major | Windowsオフセットズレ（CRLF） | 未対応 |
| [BUG-004](./BUG-004-archive-transaction.md) | main.rs | 🟠 Major | Archiveコマンドのトランザクション欠如 | 未対応 |
| [BUG-005](./BUG-005-like-injection.md) | db.rs | 🟠 Major | workspaceフィルタのLIKE特殊文字 | 未対応 |
| [BUG-006](./BUG-006-detect-code-match.md) | detect.rs | 🟡 Minor | "code"マッチが広すぎる | 未対応 |
| [BUG-007](./BUG-007-id-float.md) | mcp.rs | 🟡 Minor | id引数のfloat非対応 | 未対応 |
| [BUG-008](./BUG-008-multibyte-slice.md) | main.rs | 🟡 Minor | マルチバイト文字でパニック可能性 | 未対応 |
| [BUG-009](./BUG-009-null-cwd.md) | db.rs | 🟡 Minor | NULL cwdがワークスペーススコープで除外される | 未対応 |

## 優先度

1. BUG-001, BUG-002（データ破損リスク）
2. BUG-004（アーカイブでデータロス）
3. BUG-003（Windowsユーザー向け）
4. BUG-005, BUG-007, BUG-008（機能バグ）
5. BUG-006, BUG-009（設計上の非自明な挙動）
