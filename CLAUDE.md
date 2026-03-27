# Project Rules

## CRITICAL: Worktree Prohibition
- NEVER use EnterWorktree or EnterPlanMode tools in this project
- NEVER create git worktrees
- Always work directly in the main repository
- Do NOT enter plan mode - just read files and work directly

## Project Info
- Main plan document: PLAN.md (in project root)

## MCPツール デバッグルール

### パラメータ型のテスト原則

AIがMCPツールに渡すパラメータの型は、JSONスキーマ通りになるとは限らない。
JSON-RPC流し込みテストでは発見できないため、必ずAI自身に実際に呼ばせて確認する。

既知のパターン:
- `integer` → AIが `3.0`（float）で渡すことがある → `as_i64()` だけでは取れない
- `boolean` → AIが文字列 `"true"` で渡すことがある → `as_bool()` だけでは取れない

### 実装時の型パーサー

```rust
// 数値
let limit = args.get("limit")
    .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
    .unwrap_or(20) as usize;

// bool
let confirmed = args.get("confirmed")
    .map(|c| c.as_bool().unwrap_or_else(|| c.as_str() == Some("true")))
    .unwrap_or(false);
```

### テスト手順

1. `cargo build --release` でビルド
2. Claudeを再起動してMCPを再ロード
3. AIに実際にツールを呼ばせて動作確認（JSON-RPC流し込みは補助）
4. JSON-RPC流し込みテストは起動確認・応答確認のみに使う
