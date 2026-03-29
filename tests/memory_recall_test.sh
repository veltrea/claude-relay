#!/usr/bin/env bash
# memory_recall_test.sh — claude-relay E2E Memory Recall Test
#
# Claude CLI でフレーズを送信 → ingest → memory_search で検索 → 結果集計
#
# 使い方:
#   bash tests/memory_recall_test.sh oneshot [N]       # ワンショットモード (default: 30)
#   bash tests/memory_recall_test.sh interactive [N]   # インタラクティブモード (tmux)
#   bash tests/memory_recall_test.sh all [N]           # 両方実行
#
# 環境変数:
#   CLAUDE_BIN  — claude CLI のパス (default: /opt/homebrew/bin/claude)
#   RELAY_BIN   — claude-relay のパス (default: target/release/claude-relay)

set -euo pipefail

# ── PATH設定 ──
export PATH="/opt/homebrew/opt/node@20/bin:/opt/homebrew/opt/node/bin:/opt/homebrew/bin:$PATH"

# ── 定数 ──
MODE="${1:-help}"
NUM_PHRASES="${2:-30}"
CLAUDE_BIN="${CLAUDE_BIN:-/opt/homebrew/bin/claude}"
RELAY_BIN="${RELAY_BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/claude-relay}"
JSOL_DIR="$HOME/.claude/projects"
TIMESTAMP="$(date +%Y%m%d_%H%M%S)"
BASE_RESULTS="/tmp/claude_relay_recall"

# ── ユーティリティ ──

check_deps() {
    if [ ! -x "$RELAY_BIN" ]; then
        echo "ERROR: $RELAY_BIN が見つかりません。cargo build --release を実行してください" >&2
        exit 1
    fi
    # claude CLI チェック (node必須)
    if ! "$CLAUDE_BIN" --version &>/dev/null; then
        echo "ERROR: claude CLI が動作しません ($CLAUDE_BIN)" >&2
        echo "  node が PATH に必要です" >&2
        exit 1
    fi
}

generate_phrases() {
    local out="$1"
    local n="$2"
    for i in $(seq 1 "$n"); do
        printf "RELAY_TEST_%04d_%s\n" "$i" "$(openssl rand -hex 4)"
    done > "$out"
}

do_ingest() {
    echo ""
    echo "[Ingest] JSOL → SQLite ..."
    "$RELAY_BIN" ingest "$JSOL_DIR" 2>&1 | tail -5
    sleep 2  # WAL flush
    echo "  Done."
}

do_verify() {
    local phrases_file="$1"
    local results_dir="$2"
    local verify_log="$results_dir/verify.log"
    local found=0
    local not_found=0
    local total
    total=$(wc -l < "$phrases_file" | tr -d ' ')

    echo ""
    echo "[Verify] Searching via claude-relay tool memory_search ..."

    while IFS= read -r phrase; do
        echo -n "  [$found/$total] $phrase ... "

        result=$("$RELAY_BIN" tool memory_search --query "$phrase" --limit 5 2>&1)

        if echo "$result" | grep -q "$phrase"; then
            echo "FOUND"
            echo "OK:$phrase" >> "$results_dir/results.txt"
            found=$((found + 1))
        else
            echo "NOT_FOUND"
            echo "NG:$phrase" >> "$results_dir/results.txt"
            not_found=$((not_found + 1))
            {
                echo "--- $phrase ---"
                echo "$result"
                echo ""
            } >> "$verify_log"
        fi
    done < "$phrases_file"

    echo "$found" > "$results_dir/found_count"
    echo "$not_found" > "$results_dir/not_found_count"
}

do_report() {
    local mode_name="$1"
    local results_dir="$2"
    local phrases_file="$3"
    local seed_ok="$4"
    local seed_fail="$5"
    local summary="$results_dir/summary.txt"

    local total
    total=$(wc -l < "$phrases_file" | tr -d ' ')
    local found
    found=$(cat "$results_dir/found_count" 2>/dev/null || echo 0)
    local not_found
    not_found=$(cat "$results_dir/not_found_count" 2>/dev/null || echo 0)

    local recall_rate=0
    if [ "$total" -gt 0 ]; then
        recall_rate=$(echo "scale=1; $found * 100 / $total" | bc)
    fi

    local verdict
    if [ "$found" -eq "$total" ]; then
        verdict="PASS — 100% recall"
    elif [ "$found" -ge $((total * 9 / 10)) ]; then
        verdict="MOSTLY PASS — ${recall_rate}% recall (>90%)"
    else
        verdict="FAIL — ${recall_rate}% recall"
    fi

    cat <<EOF | tee "$summary"

=========================================
  claude-relay Memory Recall Test
  Mode: $mode_name
=========================================
Total phrases:  $total
Seed OK:        $seed_ok
Seed FAIL:      $seed_fail
Found:          $found
Not Found:      $not_found
Recall Rate:    ${recall_rate}%
Start:          $(cat "$results_dir/start_time" 2>/dev/null || echo "?")
End:            $(date)

Verdict: $verdict
=========================================
EOF

    echo ""
    echo "Results dir: $results_dir"
    [ -f "$results_dir/verify.log" ] && echo "Failed lookups: $results_dir/verify.log"
}

# ── ワンショットモード ──

run_oneshot() {
    local results_dir="$BASE_RESULTS/oneshot_${TIMESTAMP}"
    local phrases_file="$results_dir/phrases.txt"
    mkdir -p "$results_dir"
    date > "$results_dir/start_time"

    echo "=== claude-relay Memory Recall Test (Oneshot) ==="
    echo "Binary:  $RELAY_BIN"
    echo "Claude:  $CLAUDE_BIN"
    echo "Phrases: $NUM_PHRASES"
    echo "Results: $results_dir"
    echo ""

    # Phase 1: フレーズ生成
    echo "[Phase 1] Generating $NUM_PHRASES unique phrases..."
    generate_phrases "$phrases_file" "$NUM_PHRASES"

    # Phase 2: claude -p で逐次送信
    echo ""
    echo "[Phase 2] Seeding via claude -p (sequential) ..."
    local seed_ok=0
    local seed_fail=0
    local count=0

    while IFS= read -r phrase; do
        count=$((count + 1))
        echo -n "  [$count/$NUM_PHRASES] $phrase ... "

        if "$CLAUDE_BIN" -p "Please acknowledge: $phrase" \
            --output-format text \
            --max-turns 1 \
            < /dev/null \
            > "$results_dir/seed_${phrase}.txt" 2>&1; then
            echo "OK"
            seed_ok=$((seed_ok + 1))
        else
            echo "FAIL"
            seed_fail=$((seed_fail + 1))
        fi
    done < "$phrases_file"

    echo "  Seed complete: OK=$seed_ok FAIL=$seed_fail"

    # Phase 3: Ingest
    do_ingest

    # Phase 4: Verify
    do_verify "$phrases_file" "$results_dir"

    # Phase 5: Report
    do_report "oneshot" "$results_dir" "$phrases_file" "$seed_ok" "$seed_fail"
}

# ── インタラクティブモード (tmux) ──

run_interactive() {
    local results_dir="$BASE_RESULTS/interactive_${TIMESTAMP}"
    local phrases_file="$results_dir/phrases.txt"
    mkdir -p "$results_dir"
    date > "$results_dir/start_time"

    echo "=== claude-relay Memory Recall Test (Interactive / tmux) ==="
    echo "Binary:  $RELAY_BIN"
    echo "Claude:  $CLAUDE_BIN"
    echo "Phrases: $NUM_PHRASES"
    echo "Results: $results_dir"
    echo ""

    # tmux チェック
    if ! command -v tmux &>/dev/null; then
        echo "ERROR: tmux が必要です" >&2
        exit 1
    fi

    # Phase 1: フレーズ生成
    echo "[Phase 1] Generating $NUM_PHRASES unique phrases..."
    generate_phrases "$phrases_file" "$NUM_PHRASES"

    # Phase 2: tmux でインタラクティブ送信
    echo ""
    echo "[Phase 2] Seeding via tmux interactive sessions ..."
    local seed_ok=0
    local seed_fail=0
    local count=0
    local session_name="relay_recall_test"

    while IFS= read -r phrase; do
        count=$((count + 1))
        echo -n "  [$count/$NUM_PHRASES] $phrase ... "

        # 新しい tmux セッションで claude を起動
        tmux new-session -d -s "$session_name" "$CLAUDE_BIN" 2>/dev/null || {
            # 既存セッションがあれば削除して再作成
            tmux kill-session -t "$session_name" 2>/dev/null || true
            sleep 1
            tmux new-session -d -s "$session_name" "$CLAUDE_BIN"
        }

        # Claude CLI 起動待ち（MCP接続含む）
        sleep 20

        # フレーズを送信
        tmux send-keys -t "$session_name" "Please acknowledge: $phrase" Enter

        # 応答待ち
        sleep 15

        # /exit で終了
        tmux send-keys -t "$session_name" "/exit" Enter
        sleep 3

        # セッション終了確認・クリーンアップ
        tmux kill-session -t "$session_name" 2>/dev/null || true
        sleep 2

        echo "OK"
        seed_ok=$((seed_ok + 1))
    done < "$phrases_file"

    local seed_fail=$((count - seed_ok))
    echo "  Seed complete: OK=$seed_ok FAIL=$seed_fail"

    # Phase 3: Ingest
    do_ingest

    # Phase 4: Verify
    do_verify "$phrases_file" "$results_dir"

    # Phase 5: Report
    do_report "interactive" "$results_dir" "$phrases_file" "$seed_ok" "$seed_fail"
}

# ── メイン ──

case "$MODE" in
    oneshot)
        check_deps
        run_oneshot
        ;;
    interactive)
        check_deps
        run_interactive
        ;;
    all)
        check_deps
        echo "====== Running BOTH modes ======"
        echo ""
        run_oneshot
        echo ""
        echo "────────────────────────────────"
        echo ""
        run_interactive
        ;;
    *)
        cat <<EOF
usage: $0 {oneshot|interactive|all} [phrase_count]

  oneshot [N]       ワンショットモード (claude -p, default: 30)
  interactive [N]   インタラクティブモード (tmux, default: 30)
  all [N]           両方実行

examples:
  bash tests/memory_recall_test.sh oneshot 30
  bash tests/memory_recall_test.sh interactive 10
  bash tests/memory_recall_test.sh all 30
EOF
        ;;
esac
