#!/usr/bin/env bash
# test_layer3_search.sh — Layer 3: DB → memory_search → ヒット テスト
#
# DB にデータを投入し、memory_search（CLI）で正しく検索できるか検証
#
# 何を確認するか:
#   - FTS5 全文検索でユニークフレーズが見つかる
#   - 部分一致（キーワード検索）が動作する
#   - 存在しないフレーズが NOT_FOUND になる
#   - limit パラメータが機能する
#   - 日本語コンテンツが検索できる
#   - セッション一覧にテストセッションが表示される
#
# 前提: Layer 2 テストが通ること（ingest が正常動作する）
#
# 使い方: bash tests/test_layer3_search.sh

set -euo pipefail

RELAY_BIN="${RELAY_BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/claude-relay}"
DB_PATH="$HOME/.claude-relay/memory.db"
TEST_DIR="/tmp/claude_relay_layer3_test_$(date +%s)"
TEST_JSOL_DIR="$TEST_DIR/jsol"
PASS=0
FAIL=0
TOTAL=0

cleanup() {
    rm -rf "$TEST_DIR"
}
trap cleanup EXIT

assert() {
    local desc="$1"
    local result="$2"
    TOTAL=$((TOTAL + 1))
    if [ "$result" -eq 0 ]; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Layer 3 Test: DB → memory_search → ヒット ==="
echo ""

if [ ! -x "$RELAY_BIN" ]; then
    echo "ERROR: $RELAY_BIN が見つかりません" >&2
    exit 1
fi

mkdir -p "$TEST_JSOL_DIR"

# ── テストデータ投入 ──
SESSION_ID="L3TEST-$(openssl rand -hex 8)"
UNIQUE_A="L3_SEARCH_ALPHA_$(openssl rand -hex 6)"
UNIQUE_B="L3_SEARCH_BRAVO_$(openssl rand -hex 6)"
UNIQUE_JP="L3_検索テスト_$(openssl rand -hex 6)"
KEYWORD="codeword_$(openssl rand -hex 4)"

cat > "$TEST_JSOL_DIR/${SESSION_ID}.jsonl" << JSOL
{"type":"user","message":{"content":"The project $KEYWORD is $UNIQUE_A and we discussed it at length"},"timestamp":"2026-03-29T11:00:00.000Z","sessionId":"$SESSION_ID","cwd":"/tmp"}
{"type":"assistant","message":{"content":[{"type":"text","text":"I acknowledge the $KEYWORD $UNIQUE_B for future reference"}]},"timestamp":"2026-03-29T11:00:01.000Z","sessionId":"$SESSION_ID","cwd":"/tmp"}
{"type":"user","message":{"content":"日本語テスト: $UNIQUE_JP という識別子を覚えてください"},"timestamp":"2026-03-29T11:00:02.000Z","sessionId":"$SESSION_ID","cwd":"/tmp"}
{"type":"user","message":{"content":"Another entry mentioning $UNIQUE_A for duplicate testing"},"timestamp":"2026-03-29T11:00:03.000Z","sessionId":"$SESSION_ID","cwd":"/tmp"}
{"type":"user","message":{"content":"Some unrelated conversation about weather and cooking"},"timestamp":"2026-03-29T11:00:04.000Z","sessionId":"$SESSION_ID","cwd":"/tmp"}
JSOL

echo "[Setup] Ingesting test data..."
"$RELAY_BIN" ingest "$TEST_JSOL_DIR" 2>&1 | tail -3
echo ""

# ── Test 1: 完全一致検索 ──
echo "[Test 1] Exact phrase search"

RESULT_A=$("$RELAY_BIN" tool memory_search --query "$UNIQUE_A" 2>&1)
assert "find UNIQUE_A ($UNIQUE_A)" "$(echo "$RESULT_A" | grep -q "$UNIQUE_A" && echo 0 || echo 1)"

RESULT_B=$("$RELAY_BIN" tool memory_search --query "$UNIQUE_B" 2>&1)
assert "find UNIQUE_B ($UNIQUE_B)" "$(echo "$RESULT_B" | grep -q "$UNIQUE_B" && echo 0 || echo 1)"

# ── Test 2: 存在しないフレーズ ──
echo ""
echo "[Test 2] Non-existent phrase"

RESULT_NONE=$("$RELAY_BIN" tool memory_search --query "NONEXISTENT_PHRASE_$(openssl rand -hex 8)" 2>&1)
# 結果にマッチがないことを確認（"No results" や空出力）
MATCH_COUNT=$(echo "$RESULT_NONE" | grep -c "content" || true)
assert "non-existent returns 0 matches" "$([ "$MATCH_COUNT" -eq 0 ] && echo 0 || echo 1)"

# ── Test 3: 日本語検索 ──
echo ""
echo "[Test 3] Japanese content search"

RESULT_JP=$("$RELAY_BIN" tool memory_search --query "$UNIQUE_JP" 2>&1)
assert "find Japanese phrase ($UNIQUE_JP)" "$(echo "$RESULT_JP" | grep -q "$UNIQUE_JP" && echo 0 || echo 1)"

# ── Test 4: limit パラメータ ──
echo ""
echo "[Test 4] Limit parameter"

RESULT_L1=$("$RELAY_BIN" tool memory_search --query "$UNIQUE_A" --limit 1 2>&1)
assert "limit=1 returns results containing UNIQUE_A" "$(echo "$RESULT_L1" | grep -q "$UNIQUE_A" && echo 0 || echo 1)"

# ── Test 5: キーワード部分検索 ──
echo ""
echo "[Test 5] Keyword search"

RESULT_KW=$("$RELAY_BIN" tool memory_search --query "$KEYWORD" 2>&1)
assert "keyword '$KEYWORD' finds entry" "$(echo "$RESULT_KW" | grep -q "$UNIQUE_A\|$UNIQUE_B" && echo 0 || echo 1)"

# ── Test 6: セッション一覧 ──
echo ""
echo "[Test 6] Session listing"

RESULT_SESSIONS=$("$RELAY_BIN" tool memory_list_sessions 2>&1)
assert "test session appears in list ($SESSION_ID)" "$(echo "$RESULT_SESSIONS" | grep -q "$SESSION_ID" && echo 0 || echo 1)"

# ── 集計 ──
echo ""
echo "========================================="
echo "  Layer 3 Results"
echo "========================================="
echo "  PASS: $PASS / $TOTAL"
echo "  FAIL: $FAIL / $TOTAL"
echo ""

if [ "$FAIL" -eq 0 ]; then
    echo "  Verdict: PASS"
else
    echo "  Verdict: FAIL"
fi
