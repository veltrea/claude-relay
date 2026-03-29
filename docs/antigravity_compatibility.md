# Antigravity との互換性メモ

## 動作の仕組み

claude-relay が読み込むのは、Claude CLI（`claude` コマンド）が生成する JSOL ファイル（`~/.claude/projects/**/*.jsonl`）のみ。

Antigravity の会話はこの形式では記録されないため、**Antigravity 自身のセッション履歴は DB に入らない**。

## Antigravity から memory_search を呼ぶと何が起きるか

MCP サーバーとして設定されていれば、Antigravity も `memory_search` ツールを呼び出せる。ただし検索結果は **Claude の過去会話のみ**。

```
Antigravity が memory_search を呼ぶ
    ↓
Claude が過去に話した内容は検索できる
    ↓
Antigravity 自身の会話は DB に存在しない（非対称）
```

## 実用上の意味

| ユースケース | 可否 |
|---|---|
| Claude の過去会話を Antigravity から参照する | ✅ 可能 |
| Antigravity の過去会話を検索する | ❌ 不可（JSOL が生成されない） |
| セッションをまたいで Antigravity の記憶を保持する | ❌ 不可（コンテキストウィンドウのみ） |

## 結論

claude-relay は「Claude CLI が蓄積した知識を MCP 対応の任意のエージェントが参照する」一方向の橋渡しとして機能する。Antigravity から使う場合も、Claude の記憶を読む用途には有効。自分自身（Antigravity）の記憶管理には別の仕組みが必要。

## Antigravity は claude-relay を必要としない

Antigravity（IDE 統合版）は最初から会話ログの保存・検索機能を内包している。別スレッドの過去会話もツール経由で参照可能なため、claude-relay が解決しようとしている問題がそもそも存在しない。

| ツール | 記憶検索の扱い |
|---|---|
| Claude Code | セッション間の記憶なし → claude-relay が補完 |
| Antigravity | IDE が会話ログを管理・検索 → claude-relay 不要 |

claude-relay のターゲットユーザーは Claude Code ユーザーである。

