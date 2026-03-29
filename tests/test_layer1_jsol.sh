#!/usr/bin/env bash
# test_layer1_jsol.sh — Layer 1: Claude会話 → JSOL記録テスト
#
# Claude CLI (claude -p) で会話した内容が JSOL ファイルに記録されるか検証
#
# 何を確認するか:
#   - claude -p 実行後に JSOL ファイルが生成される
#   - ファイル内にユニークフレーズが含まれる
#   - JSOL の各行が有効な JSON である
#
# 使い方: bash tests/test_layer1_jsol.sh

set -euo pipefail
export PATH="/opt/homebrew/opt/node@20/bin:/opt/homebrew/opt/node/bin:/opt/homebrew/bin:$PATH"

CLAUDE_BIN="${CLAUDE_BIN:-/opt/homebrew/bin/claude}"
JSOL_DIR="$HOME/.claude/projects"
NUM_TESTS=5
PASS=0
FAIL=0

echo "=== Layer 1 Test: Claude会話 → JSOL記録 ==="
echo ""

# claude CLI 動作確認
if ! "$CLAUDE_BIN" --version &>/dev/null; then
    echo "ERROR: claude CLI が動作しません" >&2
    exit 1
fi

# JSOL ディレクトリのファイル一覧（テスト前スナップショット）
BEFORE_FILES=$(find "$JSOL_DIR" -name "*.jsonl" -type f 2>/dev/null | sort)

for i in $(seq 1 "$NUM_TESTS"); do
    PHRASE="LAYER1_TEST_$(printf "%04d" "$i")_$(openssl rand -hex 4)"
    echo -n "  [$i/$NUM_TESTS] $PHRASE ... "

    # claude -p でワンショット実行
    "$CLAUDE_BIN" -p "Please acknowledge: $PHRASE" \
        --output-format text \
        --max-turns 1 \
        < /dev/null \
        > /dev/null 2>&1 || true

    # 少し待つ（JSOL flush）
    sleep 1

    # JSOL ディレクトリで新しく出来た or 更新されたファイルを検索
    FOUND=0
    while IFS= read -r jsol_file; do
        if grep -q "$PHRASE" "$jsol_file" 2>/dev/null; then
            FOUND=1
            break
        fi
    done < <(find "$JSOL_DIR" -name "*.jsonl" -type f -newer /tmp/.layer1_marker 2>/dev/null)

    # 全ファイルからも検索（fallback）
    if [ "$FOUND" -eq 0 ]; then
        while IFS= read -r jsol_file; do
            if grep -q "$PHRASE" "$jsol_file" 2>/dev/null; then
                FOUND=1
                break
            fi
        done < <(find "$JSOL_DIR" -name "*.jsonl" -type f 2>/dev/null)
    fi

    if [ "$FOUND" -eq 1 ]; then
        echo "PASS (found in JSOL)"
        PASS=$((PASS + 1))
    else
        echo "FAIL (not found in any JSOL)"
        FAIL=$((FAIL + 1))
    fi

    # マーカー更新
    touch /tmp/.layer1_marker
done

# JSOL の JSON 妥当性チェック（最新5ファイル）
echo ""
echo "  [JSON validity] Checking latest JSOL files..."
JSON_ERRORS=0
while IFS= read -r jsol_file; do
    BAD_LINES=$(while IFS= read -r line; do
        echo "$line" | python3 -c "import json,sys; json.loads(sys.stdin.read())" 2>/dev/null || echo "BAD"
    done < "$jsol_file" | grep -c "BAD" || true)
    if [ "$BAD_LINES" -gt 0 ]; then
        echo "    WARN: $jsol_file has $BAD_LINES invalid JSON lines"
        JSON_ERRORS=$((JSON_ERRORS + BAD_LINES))
    fi
done < <(find "$JSOL_DIR" -name "*.jsonl" -type f -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -5 | awk '{print $2}')

echo ""
echo "========================================="
echo "  Layer 1 Results"
echo "========================================="
echo "  PASS: $PASS / $NUM_TESTS"
echo "  FAIL: $FAIL / $NUM_TESTS"
[ "$JSON_ERRORS" -gt 0 ] && echo "  JSON validity warnings: $JSON_ERRORS"
echo ""

if [ "$FAIL" -eq 0 ]; then
    echo "  Verdict: PASS"
else
    echo "  Verdict: FAIL"
fi

# マーカー掃除
rm -f /tmp/.layer1_marker
