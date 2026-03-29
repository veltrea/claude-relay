# BUG-001: クラッシュ時の重複挿入

**重要度**: 🔴 Critical
**ファイル**: `src/ingest.rs`
**ステータス**: 未対応

## 問題

`set_sync_offset` はループ完了後に1回だけ書かれる。
途中でプロセスがクラッシュすると、次回実行時にオフセットが0に戻り、
同じエントリが再挿入される。`raw_entries` にUNIQUE制約がないため重複を検出できない。

## 該当コード

```rust
// src/ingest.rs:38-72
for line in reader.lines() {
    let line = line?;
    let line_bytes = line.len() as i64 + 1;
    current_offset += line_bytes;
    // ... insert_entry() ...
}

// ← ここでクラッシュすると次回は offset=0 から再開
db::set_sync_offset(conn, &path_str, current_offset)?;
```

## 再現条件

1. 大きなJSONLファイルをingest中にプロセスをkill
2. 再度ingestを実行
3. → 同一エントリが2件以上DBに存在する

## 修正方針

**案A（推奨）**: バッチコミット + 定期的なoffset保存
- 100行ごとにトランザクションをコミットし、offsetも同時に保存
- クラッシュしても最大100行のやり直しで済む

**案B**: `raw_entries` にUNIQUE制約を追加
- `(session_id, timestamp, type, content の hash)` でユニーク制約
- INSERT OR IGNOREで重複をスキップ
- contentが大きいのでハッシュ化が必要

**案C**: `INSERT OR IGNORE` + content hash カラム追加
- 案Aと組み合わせが最も堅牢

## 影響範囲

- 全ユーザー（特にingest中に中断した場合）
- DBが肥大化する
- 検索結果に同一エントリが複数出現する
