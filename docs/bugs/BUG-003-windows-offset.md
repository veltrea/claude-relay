# BUG-003: Windowsオフセットズレ（CRLF）

**重要度**: 🟠 Major
**ファイル**: `src/ingest.rs`
**ステータス**: 未対応

## 問題

Windowsでは改行が `\r\n`（2バイト）だが、コードは常に `+1`（1バイト）でカウントする。
BufReaderのテキストモードが `\r` を除去するため `line.len()` は正しいが、
実際のファイルオフセットは1行あたり1バイト余計に進んでいる。

## 該当コード

```rust
// src/ingest.rs:40
let line_bytes = line.len() as i64 + 1; // +1 for newline
```

## 問題の発生条件

- Windowsでビルド・実行した場合
- Windowsで作成されたJSONLファイルをLinux/macOSで読む場合（`\r\n`が残っている）

## 修正方針

**案A**: バイナリモードでファイルを開き、`\n` を自分で探す
```rust
// BufReader<File> をバイナリで読み、\n でスプリット
// → \r\n でも \n でも正確にバイト数をカウントできる
```

**案B**: `line.len() + 1` を改行前のrawバイト列から計算
```rust
// read_until(b'\n', &mut buf) を使い buf.len() をそのままカウント
```

案Bが実装コストが低くシンプル。

## 影響範囲

- Windowsユーザー全員
- ズレが蓄積するとJSONLの途中から読み始めてパースエラーになる
- 長期運用で顕在化する（行数が多いほど深刻）
