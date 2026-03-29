# BUG-001: クラッシュ時の重複挿入

**重要度**: 🔴 Critical
**ファイル**: `src/ingest.rs`, `src/db.rs`
**ステータス**: ✅ 解決済み（バッチトランザクション導入による）

## 問題（修正前）

以前の実装では、`ingest_file` でのループが完全に完了した後に1回だけ `db::set_sync_offset` を呼び出してファイル読み込みオフセットを保存していた。

この仕組みにより、大きな JSONL ファイルを取り込んでいる最中にプロセスがクラッシュ（あるいは強制終了）すると、次回実行時に未保存のオフセットが初期値または前回の保存値から再開される。
`raw_entries` にUNIQUE制約がないため、結果としてクラッシュ前に挿入されたエントリが重複してDBに登録されてしまうという問題が発生した。

## 再現条件（修正前）

1. 大きなJSONLファイルを `ingest` コマンドで取り込み中にプロセスを kill する
2. 再度 `ingest` を実行する
3. → 同一エントリが2件以上 DB に存在する

## 修正内容

**採用方針**: バッチコミット + 定期的なoffset保存（トランザクション保護）

本問題は、データの挿入（`raw_entries` および `raw_entries_fts`）とファイルの読み出しオフセットの記録を**同一の SQLite トランザクション内**で行うバッチコミットアーキテクチャの導入により解決された。

### 実装の詳細

1. **トランザクションによるアトミック化**  
   100エントリごとに、その時点での `current_offset` を記録する `db::set_sync_offset` を呼び出した上でトランザクションを `commit()` する実装に変更。
   
2. **正確なオフセット追跡**  
   Windows の行端（CRLF）等によるバイト数のズレを防ぐため、`read_line()` で読み取った厳密なバイト数を `current_offset` に加算・保持する仕組みとした。

```rust
// src/ingest.rs 該当コード抜粋
let mut tx = conn.transaction()?;

loop {
    // ... read_lineによる1行取得、バイト数計測、JSONパース ...
    
    if let Some(n) = process_entry(&v, &file_session_id) {
        db::insert_entry(&tx, ...)?;
        count += 1;

        if count % 100 == 0 {
            // オフセット更新も同じトランザクション内で行うことでクラッシュ耐性を確保
            db::set_sync_offset(&tx, &path_str, current_offset)?;
            tx.commit()?;
            tx = conn.transaction()?; // 新しいトランザクションを開始
        }
    }
}

// 残りのトランザクションをコミット
db::set_sync_offset(&tx, &path_str, current_offset)?;
tx.commit()?;
```

これにより、プロセスが強制終了された場合でも、データベースへの挿入状態とファイルの読み込み状態（`sync_state`）の整合性が常に保たれることが保証され、最大100行程度のやり直しで安全にレジュームできる堅牢な仕組みが完成した。
