# BUG-008: マルチバイト文字でパニック可能性

**重要度**: 🟡 Minor
**ファイル**: `src/main.rs`
**ステータス**: 🟢 修正済み
## 問題

`tool memory_search` コマンドの表示処理で、contentの先頭120バイトをスライスしているが、
日本語などのマルチバイト文字の途中でスライスするとRustがパニックする。

## 該当コード（src/main.rs:392）

```rust
let preview = &e.content[..e.content.len().min(120)];
```

## 問題の例

```
content = "日本語テキスト..."
// 1文字3バイトの場合、120バイト目が文字の途中になることがある
// → パニック: byte index 120 is not a char boundary
```

## 修正指示

### 対象ファイル
`src/main.rs`

### 修正内容

`ingest.rs` にすでにある `truncate_at_char_boundary` 関数と同じロジックを使う。
ただし `truncate_at_char_boundary` は `ingest.rs` のプライベート関数なので、
`db.rs` または `main.rs` に同等の処理をインラインで書く。

```rust
// 修正前
let preview = &e.content[..e.content.len().min(120)];

// 修正後
let end = {
    let max = 120usize.min(e.content.len());
    let mut i = max;
    while i > 0 && !e.content.is_char_boundary(i) {
        i -= 1;
    }
    i
};
let preview = &e.content[..end];
```

### より簡潔な書き方（Rustイディオム）

```rust
let preview: String = e.content.chars().take(60).collect(); // 文字数で切る
```

文字数（60文字）で切る方が安全でシンプル。バイト数より意味的にも正しい。

### 変更のポイント
- `[..120]` という直接スライスをやめる
- `.chars().take(N).collect()` または `is_char_boundary` チェックで安全に切る

## テスト方法

```bash
# 日本語を含むエントリを登録してからsearchを実行
claude-relay write "これはテストです。日本語テキストです。" --type user
claude-relay tool memory_search --query "テスト"
# パニックなく表示されることを確認
```
