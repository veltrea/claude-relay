#!/bin/bash
# seed_worker.sh - 各tmuxペインで実行されるseedワーカー
# Claudeとの会話の中にテストフレーズを記録させる
#
# 引数: <pane_id> <phrases_file> <wait_seconds>

pane_id=$1
phrases_file=$2
wait_sec=${3:-15}
test_dir="${HOME}/.claude-relay-test"

# nodeのPATHを自動設定
for node_dir in /opt/homebrew/opt/node@20/bin /opt/homebrew/opt/node/bin /usr/local/bin; do
    [ -x "$node_dir/node" ] && export PATH="$node_dir:$PATH" && break
done

# claudeコマンドのパスを解決
CLAUDE_BIN=""
for candidate in \
    "/opt/homebrew/bin/claude" \
    "/usr/local/bin/claude" \
    "${HOME}/.local/bin/claude"; do
    if [ -x "$candidate" ]; then
        CLAUDE_BIN="$candidate"
        break
    fi
done

if [ -z "$CLAUDE_BIN" ]; then
    echo "[pane $pane_id] ERROR: claude コマンドが見つかりません" >&2
    touch "${test_dir}/seed_done_${pane_id}.flag"
    exit 1
fi

phrases=()
while IFS= read -r line; do
    [ -n "$line" ] && phrases+=("$line")
done < "$phrases_file"

echo "[pane $pane_id] claude=$CLAUDE_BIN phrases=${#phrases[@]}"

for phrase in "${phrases[@]}"; do
    echo "[pane $pane_id] seeding: $phrase"
    "$CLAUDE_BIN" -p "記憶テスト: 次のコードを確認してください: ${phrase}" 2>&1 | tail -1 || true
    sleep "$wait_sec"
done

touch "${test_dir}/seed_done_${pane_id}.flag"
echo "[pane $pane_id] done"
