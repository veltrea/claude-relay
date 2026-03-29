# BUG-004: Archiveコマンドのトランザクション欠如

**重要度**: 🟠 Major
**ファイル**: `src/main.rs`
**ステータス**: 未対応

## 問題

`archive` コマンドが「ファイル書き込み → DBから削除」をトランザクションなしで実行する。
書き込み後・削除前にクラッシュすると、次回 `archive` が同じ日付を再アーカイブして
ファイルが上書きされる。逆に削除後・次の日付処理前にクラッシュすると
その日のデータがDBからもファイルからも消える可能性がある。

## 該当コード（src/main.rs:215-223）

```rust
let md = export::export_date(&conn, date)?;
std::fs::create_dir_all(&dir)?;
std::fs::write(&file, &md)?;          // ← ファイル書き込み
conn.execute(                          // ← DB削除（別操作）
    "DELETE FROM raw_entries WHERE date = ?1",
    rusqlite::params![date],
)?;
```

## 修正指示

### 対象ファイル
`src/main.rs` の `Commands::Archive` ブランチ（おおよそ183〜234行目）

### 修正内容

各日付の処理を `conn.transaction()` で囲む。
ファイル書き込みは先に行い、DBトランザクション内でDELETEする。

```rust
// 修正後のイメージ
for date in &dates {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 { continue; }
    let dir = archive_dir.join(parts[0]).join(parts[1]);
    let file = dir.join(format!("{}.md", parts[2]));

    if dry {
        println!("[dry] Would archive {date} -> {}", file.display());
    } else {
        let md = export::export_date(&conn, date)?;
        std::fs::create_dir_all(&dir)?;

        // ファイルが既に存在する場合は上書きしない（冪等性）
        if !file.exists() {
            std::fs::write(&file, &md)?;
        }

        // DELETE はファイル確定後にトランザクションで実行
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM raw_entries WHERE date = ?1",
            rusqlite::params![date],
        )?;
        tx.commit()?;

        println!("Archived {date} -> {}", file.display());
    }
}
```

### 変更のポイント
1. `conn.transaction()` でDELETEを囲む
2. `file.exists()` チェックで再実行時の上書きを防ぐ（冪等性）
3. それ以外のロジックは変更しない

## テスト方法

1. `cargo build --release` でビルド
2. `claude-relay archive --dry` で影響範囲確認
3. `claude-relay archive` 実行後、DBとファイルの整合性を確認
