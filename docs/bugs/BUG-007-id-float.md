# BUG-007: id引数のfloat非対応

**重要度**: 🟡 Minor
**ファイル**: `src/mcp.rs`
**ステータス**: 解決済み

## 問題

AIが `memory_get_entry` ツールに `id` を渡す際、`3` ではなく `3.0`（float）で
渡すことがある（既知のAI挙動パターン）。
`as_i64()` はfloatを受け付けないため `id=0` として扱われ、意図したエントリが取得できない。

`limit` には既にfloat対応が書かれているが、`id` には書かれていない。

## 該当コード（src/mcp.rs 内の memory_get_entry 処理）

```rust
// 現状（問題あり）
let id = args.get("id").and_then(|i| i.as_i64()).unwrap_or(0);
```

## 修正指示

### 対象ファイル
`src/mcp.rs`

### 修正内容

`limit` の処理と同じパターンを `id` にも適用する。

```rust
// 修正前
let id = args.get("id").and_then(|i| i.as_i64()).unwrap_or(0);

// 修正後
let id = args.get("id")
    .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
    .unwrap_or(0);
```

### 変更のポイント
- `as_i64()` で取れなかった場合に `as_f64().map(|f| f as i64)` でfloatとして試みる
- `limit` の修正（CLAUDE.mdに記載済み）と同じパターン

## テスト方法

CLAUDE.mdの「テスト手順」に従い、AIに実際に `memory_get_entry` を呼ばせて確認。

```
1. cargo build --release
2. Claudeを再起動してMCPを再ロード
3. Claudeに「memory_get_entry id=1 で取得して」と依頼
4. 正しいエントリが返ってくることを確認
```

## 参考

`CLAUDE.md` の「MCPツール デバッグルール」に記載されているパターン：

```rust
let limit = args.get("limit")
    .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)))
    .unwrap_or(20) as usize;
```
