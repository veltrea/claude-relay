# BUG-002: FTS/raw_entriesのトランザクション欠如

**重要度**: 🔴 Critical
**ファイル**: `src/db.rs`
**ステータス**: ✅ 解決済み（バッチトランザクション導入による）

## 問題

`insert_entry` 内で `raw_entries` と `raw_entries_fts` への2つのINSERTが
トランザクションで囲まれていない。
1つ目が成功して2つ目が失敗した場合、FTSに存在しないエントリが生まれ、
`memory_search` でそのエントリが永久に検索できなくなる。

## 該当コード

```rust
// src/db.rs:100-116
pub fn insert_entry(...) -> Result<i64> {
    conn.execute(
        "INSERT INTO raw_entries ...",  // ← 1回目
        ...
    )?;
    let id = conn.last_insert_rowid();

    // ← ここでクラッシュ/エラーが起きると raw_entries だけに入る
    conn.execute(
        "INSERT INTO raw_entries_fts ...",  // ← 2回目
        ...
    )?;

    Ok(id)
}
```

## 修正方針

`insert_entry` の呼び出し元（`ingest_file`）全体をトランザクションで囲む。
または `insert_entry` 内で BEGIN/COMMIT する（ただしネスト注意）。

```rust
// 修正イメージ
let tx = conn.transaction()?;
// ... INSERT raw_entries
// ... INSERT raw_entries_fts
tx.commit()?;
```

BUG-001の修正（バッチトランザクション）と同時に対応するのが自然。

## 影響範囲

- `memory_search` が特定エントリを返さない
- `raw_entries` と `raw_entries_fts` の件数が一致しなくなる
- `db vacuum` や `archive` 後のFTS再構築で回復可能だが、
  通常運用では気づきにくい
