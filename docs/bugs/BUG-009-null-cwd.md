# BUG-009: NULL cwdがワークスペーススコープで除外される

**重要度**: 🟡 Minor
**ファイル**: `src/db.rs`
**ステータス**: ✅ 修正済み（2026-03-30）

## 問題

workspaceフィルタは `cwd LIKE 'path%'` で絞り込むが、
SQLでは `NULL LIKE anything` はNULL（偽）になる。
そのため `cwd` が記録されていないエントリが全て除外される。

特に古いバージョンで取り込まれたエントリや、
cwdが取得できなかった環境のエントリが見えなくなる。

## 該当コード（src/db.rs:266-270）

```rust
if let Some(ws) = workspace {
    conditions.push(format!("{} LIKE ?{param_idx}", col("cwd")));
    param_values.push(Box::new(format!("{ws}%")));
    param_idx += 1;
}
```

## 修正指示

### 対象ファイル
`src/db.rs`

### 修正内容

`cwd IS NULL` の場合も含めるかどうかは設計判断だが、
現状は意図が不明確なので、NULL cwdを含める方向に修正することを推奨する。

```rust
// 修正前
conditions.push(format!("{} LIKE ?{param_idx}", col("cwd")));

// 修正後（NULLも含める場合）
conditions.push(format!(
    "({col} LIKE ?{param_idx} ESCAPE '\\' OR {col} IS NULL)",
    col = col("cwd")
));
```

または、NULLを除外する現在の挙動を明示的なドキュメントコメントで説明するだけでも改善になる。

```rust
// cwd IS NULL のエントリはワークスペーススコープから除外される（意図的）
// 古いエントリやcwd未取得エントリは cross-scope で検索可能
conditions.push(format!("{} LIKE ?{param_idx} ESCAPE '\\'", col("cwd")));
```

### 変更のポイント
- NULLを含めるか除外するかをチームで判断して明示する
- BUG-005（LIKE injection修正）と同時に対応するのが効率的

## テスト方法

```bash
# cwd=NULL のエントリを作成
claude-relay write "cwdなしエントリ" --type user

# ワークスペーススコープで検索したとき、このエントリが含まれるかを確認
claude-relay tool memory_search --query "cwdなし"
```
