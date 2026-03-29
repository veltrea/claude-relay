# BUG-005: workspaceフィルタのLIKE特殊文字

**重要度**: 🟠 Major
**ファイル**: `src/db.rs`
**ステータス**: 🟢 対応済

## 問題

workspaceパスに `%` や `_` が含まれる場合、SQLの `LIKE` 演算子のワイルドカードとして
解釈されてしまい、意図しないレコードがマッチする。

例: `/home/user/proj_a` というパスで検索すると
`_` がワイルドカードとなり `/home/user/projXa` も一致してしまう。

## 該当コード（src/db.rs:266-270）

```rust
if let Some(ws) = workspace {
    conditions.push(format!("{} LIKE ?{param_idx}", col("cwd")));
    param_values.push(Box::new(format!("{ws}%")));  // ← ws内の%や_がエスケープされない
    param_idx += 1;
}
```

`list_sessions` 関数（src/db.rs:345-349）にも同じ問題がある。

## 修正指示

### 対象ファイル
`src/db.rs`

### 修正箇所1: `search` 関数（266行目付近）

```rust
// 修正前
if let Some(ws) = workspace {
    conditions.push(format!("{} LIKE ?{param_idx}", col("cwd")));
    param_values.push(Box::new(format!("{ws}%")));
    param_idx += 1;
}

// 修正後
if let Some(ws) = workspace {
    // % と _ と \ をエスケープする
    let escaped = ws.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    conditions.push(format!("{} LIKE ?{param_idx} ESCAPE '\\'", col("cwd")));
    param_values.push(Box::new(format!("{escaped}%")));
    param_idx += 1;
}
```

### 修正箇所2: `list_sessions` 関数（345行目付近）

同様に修正する。

```rust
// 修正前
if let Some(ws) = workspace {
    conditions.push(format!("cwd LIKE ?{idx}"));
    params_vec.push(Box::new(format!("{ws}%")));
    idx += 1;
}

// 修正後
if let Some(ws) = workspace {
    let escaped = ws.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    conditions.push(format!("cwd LIKE ?{idx} ESCAPE '\\'"));
    params_vec.push(Box::new(format!("{escaped}%")));
    idx += 1;
}
```

### エスケープのルール
- `\` → `\\`（バックスラッシュ自体をエスケープ、必ず最初に処理）
- `%` → `\%`（パーセント）
- `_` → `\_`（アンダースコア）
- SQLに `ESCAPE '\\'` を追加してエスケープ文字を宣言

## テスト方法

アンダースコアを含むパスで検索して、意図しないレコードがヒットしないことを確認。

```bash
claude-relay tool memory_search --query ""
# cwdに _ を含むパスで登録されたエントリだけが出ること
```
