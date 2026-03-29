# BUG-006: detect.rsの"code"マッチが広すぎる

**重要度**: 🟡 Minor
**ファイル**: `src/detect.rs`
**ステータス**: 未対応

## 問題

`normalize_client` 関数の `"code"` チェックが広すぎる。
`codium`（VSCodium）、`code-server`、`recode` など
"code" を含む無関係なプロセスが全て `"vscode"` と誤認識される。

また macOS の `ps -o comm=` はプロセス名を約15文字で切り詰めるため、
`claude-code` の親プロセスが `node` や `Electron` として見えることが多く、
PPID検出そのものが当てにならないケースがある。

## 該当コード（src/detect.rs:84）

```rust
} else if n.contains("code") || n.contains("vscode") {
    "vscode"
}
```

## 修正指示

### 対象ファイル
`src/detect.rs`

### 修正内容

`"code"` の前方一致チェックをより厳密にする。

```rust
// 修正前
} else if n.contains("code") || n.contains("vscode") {
    "vscode"
}

// 修正後
} else if n == "code" || n.starts_with("code ") || n.contains("vscode") || n == "codium" {
    "vscode"
}
```

### 変更のポイント
- `n.contains("code")` → `n == "code"` または `n.starts_with("code ")` に変更
- `codium`（VSCodium）を明示的に追加
- `code-server` は意図的にマッチさせたい場合は `n.starts_with("code-server")` を追加

## テスト方法

```bash
# detect.rs の normalize_client を直接テストする単体テストを追加
# または claude-relay serve を起動してログのclientフィールドを確認
```

## 補足

PPID経由のクライアント検出は補助的な手段であり、
MCP `initialize` の `clientInfo.name` が取得できた場合はそちらが優先される（`mcp.rs` 参照）。
この修正は `clientInfo` が取得できなかった場合のフォールバックの精度改善。
