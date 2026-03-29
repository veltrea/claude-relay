#!/usr/bin/env bash
# test_layer2_ingest.sh — Layer 2: JSOL → ingest → DB テスト
#
# 合成 JSOL ファイルを作成し、ingest で DB に格納されるか検証
#
# 何を確認するか:
#   - 合成 JSOL を ingest すると DB にエントリが追加される
#   - user / assistant / system 各タイプが正しく格納される
#   - FTS5 テーブルにも content が登録される
#   - 差分 ingest（2回目は追加されない）が正しく動作する
#   - 壊れた JSON 行がスキップされ後続行が正常処理される
#
# 注意: 本番 DB (~/.claude-relay/memory.db) にテストデータが追加されるが
#       ユニークなプレフィックス付きなので干渉しない
#
# 使い方: bash tests/test_layer2_ingest.sh

set -euo pipefail

RELAY_BIN="${RELAY_BIN:-$(cd "$(dirname "$0")/.." && pwd)/target/release/claude-relay}"
DB_PATH="$HOME/.claude-relay/memory.db"
TEST_DIR="/tmp/claude_relay_layer2_test_$(date +%s)"
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
    local result="$2"   # 0=pass, nonzero=fail
    TOTAL=$((TOTAL + 1))
    if [ "$result" -eq 0 ]; then
        echo "  PASS: $desc"
        PASS=$((PASS + 1))
    else
        echo "  FAIL: $desc"
        FAIL=$((FAIL + 1))
    fi
}

echo "=== Layer 2 Test: JSOL → ingest → DB ==="
echo ""

if [ ! -x "$RELAY_BIN" ]; then
    echo "ERROR: $RELAY_BIN が見つかりません" >&2
    exit 1
fi

mkdir -p "$TEST_JSOL_DIR"

# ── 合成 JSOL の生成 ──
SESSION_ID="L2TEST-$(openssl rand -hex 8)"
UNIQUE_USER="L2_INGEST_USER_$(openssl rand -hex 6)"
UNIQUE_ASST="L2_INGEST_ASST_$(openssl rand -hex 6)"
UNIQUE_SYS="L2_INGEST_SYS_$(openssl rand -hex 6)"

cat > "$TEST_JSOL_DIR/${SESSION_ID}.jsonl" << JSOL
{"type":"user","message":{"content":"$UNIQUE_USER"},"timestamp":"2026-03-29T10:00:00.000Z","sessionId":"$SESSION_ID","cwd":"/tmp","gitBranch":"main"}
{"type":"assistant","message":{"content":[{"type":"text","text":"$UNIQUE_ASST"}]},"timestamp":"2026-03-29T10:00:01.000Z","sessionId":"$SESSION_ID","cwd":"/tmp","gitBranch":"main"}
{"type":"system","message":{"content":"$UNIQUE_SYS"},"timestamp":"2026-03-29T10:00:02.000Z","sessionId":"$SESSION_ID"}
JSOL

# ingest 前の件数を記録
BEFORE_COUNT=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries WHERE session_id='$SESSION_ID';" 2>/dev/null || echo 0)

echo "[Test 1] Basic ingest"
OUTPUT=$("$RELAY_BIN" ingest "$TEST_JSOL_DIR" 2>&1)
echo "  $OUTPUT" | tail -3

COUNT=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries WHERE session_id='$SESSION_ID';")
EXPECTED=$((BEFORE_COUNT + 3))
assert "3 entries inserted (count=$COUNT)" "$([ "$COUNT" -eq "$EXPECTED" ] && echo 0 || echo 1)"

# user エントリの content 確認
USER_CONTENT=$(sqlite3 "$DB_PATH" "SELECT content FROM raw_entries WHERE session_id='$SESSION_ID' AND type='user' LIMIT 1;")
assert "user content contains unique phrase" "$(echo "$USER_CONTENT" | grep -q "$UNIQUE_USER" && echo 0 || echo 1)"

# assistant エントリの content 確認
ASST_CONTENT=$(sqlite3 "$DB_PATH" "SELECT content FROM raw_entries WHERE session_id='$SESSION_ID' AND type='assistant';")
assert "assistant content contains unique phrase" "$(echo "$ASST_CONTENT" | grep -q "$UNIQUE_ASST" && echo 0 || echo 1)"

# FTS5 テーブルにも存在するか
FTS_COUNT=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries_fts WHERE content MATCH '\"$UNIQUE_USER\"';")
assert "FTS5 contains user content" "$([ "$FTS_COUNT" -ge 1 ] && echo 0 || echo 1)"

echo ""
echo "[Test 2] Idempotent ingest (差分: 同じファイルの再取り込み)"
OUTPUT2=$("$RELAY_BIN" ingest "$TEST_JSOL_DIR" 2>&1)
COUNT2=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries WHERE session_id='$SESSION_ID';")
assert "no duplicates after re-ingest (still $EXPECTED)" "$([ "$COUNT2" -eq "$EXPECTED" ] && echo 0 || echo 1)"

echo ""
echo "[Test 3] Append ingest (JSOL に行追加)"
UNIQUE_APPEND="L2_INGEST_APPEND_$(openssl rand -hex 6)"
echo "{\"type\":\"user\",\"message\":{\"content\":\"$UNIQUE_APPEND\"},\"timestamp\":\"2026-03-29T10:00:03.000Z\",\"sessionId\":\"$SESSION_ID\",\"cwd\":\"/tmp\"}" >> "$TEST_JSOL_DIR/${SESSION_ID}.jsonl"
OUTPUT3=$("$RELAY_BIN" ingest "$TEST_JSOL_DIR" 2>&1)
COUNT3=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries WHERE session_id='$SESSION_ID';")
assert "append adds 1 more (now $((EXPECTED + 1)))" "$([ "$COUNT3" -eq $((EXPECTED + 1)) ] && echo 0 || echo 1)"

APPEND_FTS=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries_fts WHERE content MATCH '\"$UNIQUE_APPEND\"';")
assert "appended entry in FTS5" "$([ "$APPEND_FTS" -ge 1 ] && echo 0 || echo 1)"

echo ""
echo "[Test 4] Invalid JSON handling"
echo "THIS IS NOT JSON {broken" >> "$TEST_JSOL_DIR/${SESSION_ID}.jsonl"
UNIQUE_AFTER="L2_INGEST_AFTER_$(openssl rand -hex 6)"
echo "{\"type\":\"user\",\"message\":{\"content\":\"$UNIQUE_AFTER\"},\"timestamp\":\"2026-03-29T10:00:04.000Z\",\"sessionId\":\"$SESSION_ID\"}" >> "$TEST_JSOL_DIR/${SESSION_ID}.jsonl"
OUTPUT4=$("$RELAY_BIN" ingest "$TEST_JSOL_DIR" 2>&1)
COUNT4=$(sqlite3 "$DB_PATH" "SELECT COUNT(*) FROM raw_entries WHERE session_id='$SESSION_ID';")
assert "broken JSON skipped, valid line ingested (now $((EXPECTED + 2)))" "$([ "$COUNT4" -eq $((EXPECTED + 2)) ] && echo 0 || echo 1)"

echo ""
echo "[Test 5] Entry types stored correctly"
TYPES=$(sqlite3 "$DB_PATH" "SELECT DISTINCT type FROM raw_entries WHERE session_id='$SESSION_ID' ORDER BY type;")
assert "has user type" "$(echo "$TYPES" | grep -q "user" && echo 0 || echo 1)"
assert "has assistant type" "$(echo "$TYPES" | grep -q "assistant" && echo 0 || echo 1)"
assert "has system type" "$(echo "$TYPES" | grep -q "system" && echo 0 || echo 1)"

# ── 集計 ──
echo ""
echo "========================================="
echo "  Layer 2 Results"
echo "========================================="
echo "  PASS: $PASS / $TOTAL"
echo "  FAIL: $FAIL / $TOTAL"
echo ""

if [ "$FAIL" -eq 0 ]; then
    echo "  Verdict: PASS"
else
    echo "  Verdict: FAIL"
fi
