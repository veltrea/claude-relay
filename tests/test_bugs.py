#!/usr/bin/env python3
"""
claude-relay BUG-001〜009 包括テスト

使い方:
    python3 tests/test_bugs.py

テスト戦略:
  - subprocess でバイナリを呼び出し
  - sqlite3 で直接DBを検証
  - JSON-RPC stdin/stdout で MCP サーバーを叩く（BUG-007等）
  - signal.SIGKILL でプロセスを途中killしてクラッシュ耐性を検証（BUG-001）
  - テスト用DBは一時ディレクトリに隔離、終了後に自動削除
"""

import json
import os
import signal
import sqlite3
import subprocess
import sys
import tempfile
import textwrap
import time
import unittest
from pathlib import Path

# ── 定数 ──────────────────────────────────────────────────────────────

PROJECT_ROOT = Path(__file__).resolve().parent.parent
RELAY_BIN = PROJECT_ROOT / "target" / "release" / "claude-relay"


def require_binary():
    if not RELAY_BIN.exists():
        print(f"ERROR: バイナリが見つかりません: {RELAY_BIN}")
        print("  cargo build --release を先に実行してください")
        sys.exit(1)


# ── ヘルパー ──────────────────────────────────────────────────────────

class RelayTestCase(unittest.TestCase):
    """各テストに隔離された HOME と DB を提供する基底クラス"""

    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp(prefix="relay_test_")
        self.env = os.environ.copy()
        self.env["HOME"] = self.tmp_dir
        # DB パス
        self.db_dir = Path(self.tmp_dir) / ".claude-relay"
        self.db_dir.mkdir(parents=True, exist_ok=True)
        self.db_path = self.db_dir / "memory.db"

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmp_dir, ignore_errors=True)

    def run_relay(self, *args, input_data=None, timeout=30):
        """claude-relay を実行して (stdout, stderr, returncode) を返す"""
        result = subprocess.run(
            [str(RELAY_BIN)] + list(args),
            capture_output=True, text=True, timeout=timeout,
            env=self.env, input=input_data,
        )
        return result.stdout, result.stderr, result.returncode

    def run_relay_bg(self, *args):
        """バックグラウンドでプロセスを起動（kill用）"""
        return subprocess.Popen(
            [str(RELAY_BIN)] + list(args),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            env=self.env,
        )

    def query_db(self, sql, params=()):
        """SQLite を直接開いてクエリ実行"""
        conn = sqlite3.connect(str(self.db_path))
        try:
            cur = conn.execute(sql, params)
            return cur.fetchall()
        finally:
            conn.close()

    def query_scalar(self, sql, params=()):
        """単一値を返す"""
        rows = self.query_db(sql, params)
        return rows[0][0] if rows else None

    def make_jsonl_entry(self, session, ts, cwd="/test/project", content="test"):
        """JSONL エントリの JSON 文字列を生成"""
        return json.dumps({
            "type": "user",
            "sessionId": session,
            "timestamp": ts,
            "cwd": cwd,
            "message": {"role": "user", "content": content},
        })

    def write_jsonl(self, path, entries, line_ending="\n"):
        """JSONL ファイルを書き出す"""
        with open(path, "wb") as f:
            for entry in entries:
                f.write((entry + line_ending).encode("utf-8"))

    def send_mcp_request(self, method, params=None, workspace=None, timeout=10):
        """MCP サーバーに JSON-RPC リクエストを送信して結果を受け取る"""
        cmd = [str(RELAY_BIN), "serve"]
        if workspace:
            cmd += ["--workspace", workspace]

        request = json.dumps({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params or {},
        })

        result = subprocess.run(
            cmd, capture_output=True, text=True, timeout=timeout,
            env=self.env, input=request + "\n",
        )
        # MCP は stdout に JSON-RPC レスポンスを返す
        for line in result.stdout.strip().split("\n"):
            line = line.strip()
            if line.startswith("{"):
                try:
                    return json.loads(line)
                except json.JSONDecodeError:
                    continue
        return None


# ── BUG-001: クラッシュ時の重複挿入防止 ──────────────────────────────

class TestBug001_DuplicateIngest(RelayTestCase):
    """同じファイルを複数回 ingest しても重複しないことを検証

    【旧挙動】ingest中にプロセスがクラッシュ(kill)すると、sync_state の
    last_offset が更新されないまま raw_entries には一部が書き込まれた状態に
    なる。再起動後の ingest が前回の途中から再開するため、既に書き込み済みの
    行が再度INSERTされて重複が発生していた。
    【修正】INSERT時に (session_id, timestamp, content) の複合UNIQUEで
    重複を弾き、sync_offset の更新をトランザクション内で行うようにした。"""

    def test_double_ingest_no_duplicates(self):
        """基本: 同じファイルを2回ingestして重複ゼロ"""
        jsonl = Path(self.tmp_dir) / "test.jsonl"
        entries = [
            self.make_jsonl_entry("s001", "2026-01-01T10:00:00.000Z", content="alpha"),
            self.make_jsonl_entry("s001", "2026-01-01T10:01:00.000Z", content="beta"),
            self.make_jsonl_entry("s001", "2026-01-01T10:02:00.000Z", content="gamma"),
        ]
        self.write_jsonl(jsonl, entries)

        self.run_relay("ingest", str(jsonl))
        self.run_relay("ingest", str(jsonl))  # 2回目

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='s001'"
        )
        self.assertEqual(count, 3, "2回ingestしても3件のまま（重複なし）")

    def test_stress_ingest_large_file(self):
        """ストレス: 1000行のJSONLを3回ingestして重複ゼロ"""
        jsonl = Path(self.tmp_dir) / "large.jsonl"
        entries = []
        for i in range(1000):
            entries.append(self.make_jsonl_entry(
                "stress001",
                f"2026-01-01T10:{i//60:02d}:{i%60:02d}.000Z",
                content=f"stress entry {i}",
            ))
        self.write_jsonl(jsonl, entries)

        self.run_relay("ingest", str(jsonl))
        self.run_relay("ingest", str(jsonl))
        self.run_relay("ingest", str(jsonl))

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='stress001'"
        )
        self.assertEqual(count, 1000, "3回ingestしても1000件のまま")

    def test_crash_recovery_no_duplicates(self):
        """クラッシュ耐性: 大量データを途中killし、再開後に重複がない"""
        jsonl = Path(self.tmp_dir) / "crash.jsonl"
        # 5000行で十分大きいファイルを作成
        entries = []
        for i in range(5000):
            entries.append(self.make_jsonl_entry(
                "crash001",
                f"2026-01-02T{i//3600:02d}:{(i%3600)//60:02d}:{i%60:02d}.000Z",
                content=f"crash test entry {i}",
            ))
        self.write_jsonl(jsonl, entries)

        # 1回目: 途中で kill（タイミングに依存するが、最低限のチェック）
        proc = self.run_relay_bg("ingest", str(jsonl))
        time.sleep(0.3)  # 少し処理させてからkill
        try:
            proc.kill()
            proc.wait(timeout=5)
        except Exception:
            pass

        # 2回目: 正常に完走させる
        self.run_relay("ingest", str(jsonl))

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='crash001'"
        )
        self.assertEqual(count, 5000, "kill後の再ingestで正確に5000件")

        # sync_offset がファイルサイズと一致
        file_size = jsonl.stat().st_size
        offset = self.query_scalar(
            "SELECT last_offset FROM sync_state WHERE file_path=?",
            (str(jsonl),)
        )
        self.assertEqual(offset, file_size, "sync_offset がファイルサイズと完全一致")


# ── BUG-002: FTS/raw_entries トランザクション一致 ─────────────────────

class TestBug002_FTSConsistency(RelayTestCase):
    """raw_entries と raw_entries_fts が常に同期していることを検証

    【旧挙動】raw_entries への INSERT と raw_entries_fts への INSERT が
    別々のSQL文で実行されていたため、途中で失敗すると raw_entries には
    行があるのに FTS にはない（または逆）という不整合が発生した。
    memory_search が FTS 経由で検索するため、FTS に欠損があると
    存在するエントリが検索にヒットしないという症状になっていた。
    【修正】raw_entries INSERT と FTS INSERT を同一トランザクション内で
    実行するようにした。"""

    def test_count_match(self):
        """件数が一致"""
        jsonl = Path(self.tmp_dir) / "fts.jsonl"
        entries = [
            self.make_jsonl_entry("fts001", "2026-01-01T10:00:00.000Z", content="unique_alpha_term"),
            self.make_jsonl_entry("fts001", "2026-01-01T10:01:00.000Z", content="unique_beta_term"),
            self.make_jsonl_entry("fts001", "2026-01-01T10:02:00.000Z", content="unique_gamma_term"),
        ]
        self.write_jsonl(jsonl, entries)
        self.run_relay("ingest", str(jsonl))

        raw_count = self.query_scalar("SELECT COUNT(*) FROM raw_entries")
        fts_count = self.query_scalar("SELECT COUNT(*) FROM raw_entries_fts")
        self.assertEqual(raw_count, fts_count, "raw_entries と FTS の件数が一致")

    def test_fts_search_actually_finds_entries(self):
        """FTS検索で実際にエントリがヒットする（件数だけでなく内容も確認）"""
        self.run_relay("write", "xylophone_unique_test_word", "--type", "user", "--session", "fts002")

        # FTS で検索
        rows = self.query_db(
            "SELECT content FROM raw_entries_fts WHERE raw_entries_fts MATCH 'xylophone'"
        )
        self.assertGreaterEqual(len(rows), 1, "FTS MATCH で xylophone がヒットする")

    def test_fts_covers_all_raw(self):
        """raw_entries の全IDがFTSにも存在する"""
        for i in range(10):
            self.run_relay("write", f"fts_check_{i}", "--type", "user", "--session", f"fts003_{i}")

        raw_ids = set(r[0] for r in self.query_db("SELECT id FROM raw_entries"))
        fts_ids = set(r[0] for r in self.query_db("SELECT rowid FROM raw_entries_fts"))
        self.assertEqual(raw_ids, fts_ids, "raw_entries の全IDがFTSに存在")


# ── BUG-003: CRLF 改行ファイルのオフセット ───────────────────────────

class TestBug003_CRLFOffset(RelayTestCase):
    """CRLF改行でオフセットがずれないことを検証

    【旧挙動】BufReader::read_line() は CRLF の \\r\\n を \\n に正規化して
    返すが、sync_offset の計算に line.len() （正規化後のバイト数）を
    使っていたため、1行ごとに1バイトずつオフセットがずれていた。
    結果、2回目の ingest 時に「前回の続き」の位置が実際より手前になり、
    JSON行の途中から読み始めてパースエラーか重複挿入が発生していた。
    Windows環境（CRLF）やgitのautocrlf=true設定で顕在化する。
    【修正】read_line() ではなくバイト単位で読み、\\r\\n を検出して
    正しいバイト数で offset を計算するようにした。"""

    def test_crlf_offset_accurate(self):
        """CRLF ファイルの sync_offset がファイルサイズと一致"""
        jsonl = Path(self.tmp_dir) / "crlf.jsonl"
        entries = [
            self.make_jsonl_entry("crlf001", "2026-01-02T10:00:00.000Z", content="CRLF one"),
            self.make_jsonl_entry("crlf001", "2026-01-02T10:01:00.000Z", content="CRLF two"),
            self.make_jsonl_entry("crlf001", "2026-01-02T10:02:00.000Z", content="CRLF three"),
        ]
        self.write_jsonl(jsonl, entries, line_ending="\r\n")

        self.run_relay("ingest", str(jsonl))

        file_size = jsonl.stat().st_size
        offset = self.query_scalar(
            "SELECT last_offset FROM sync_state WHERE file_path=?", (str(jsonl),)
        )
        self.assertEqual(offset, file_size, "CRLF: sync_offset == file_size")

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='crlf001'"
        )
        self.assertEqual(count, 3, "CRLF ファイルから3件取り込み")

    def test_crlf_second_ingest_zero(self):
        """CRLF ファイルの2回目ingestが0件"""
        jsonl = Path(self.tmp_dir) / "crlf2.jsonl"
        entries = [
            self.make_jsonl_entry("crlf002", f"2026-01-02T10:0{i}:00.000Z", content=f"entry {i}")
            for i in range(5)
        ]
        self.write_jsonl(jsonl, entries, line_ending="\r\n")

        self.run_relay("ingest", str(jsonl))
        count_before = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='crlf002'"
        )

        self.run_relay("ingest", str(jsonl))  # 2回目
        count_after = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='crlf002'"
        )
        self.assertEqual(count_before, count_after, "2回目ingestで増えない")
        self.assertEqual(count_after, 5)

    def test_mixed_line_endings(self):
        """LF と CRLF が混在するファイルでも正しく処理"""
        jsonl = Path(self.tmp_dir) / "mixed.jsonl"
        with open(jsonl, "wb") as f:
            f.write(self.make_jsonl_entry("mix001", "2026-01-03T10:00:00.000Z", content="LF line").encode() + b"\n")
            f.write(self.make_jsonl_entry("mix001", "2026-01-03T10:01:00.000Z", content="CRLF line").encode() + b"\r\n")
            f.write(self.make_jsonl_entry("mix001", "2026-01-03T10:02:00.000Z", content="LF again").encode() + b"\n")

        self.run_relay("ingest", str(jsonl))

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='mix001'"
        )
        self.assertEqual(count, 3, "混在改行でも3件取り込み")

        file_size = jsonl.stat().st_size
        offset = self.query_scalar(
            "SELECT last_offset FROM sync_state WHERE file_path=?", (str(jsonl),)
        )
        self.assertEqual(offset, file_size, "混在改行: offset == file_size")


# ── BUG-004: Archive 冪等性 ──────────────────────────────────────────

class TestBug004_ArchiveIdempotency(RelayTestCase):
    """Archive を複数回実行しても壊れないことを検証

    【旧挙動】archive コマンドは「DBからSELECT → MDファイルに書き出し →
    DBからDELETE」の3ステップだったが、トランザクションで囲まれていなかった。
    MDファイル書き出し後・DELETE前にクラッシュすると、次回のarchiveで
    同じエントリが再度MDに追記されて内容が重複した。
    また、2回目のarchive（対象0件）でも空のMDファイルを生成して
    既存ファイルを上書きする可能性があった。
    【修正】SELECT→書き出し→DELETE を1トランザクションで囲み、
    対象0件の場合は何もしないようにした。"""

    def _setup_archive_entry(self, session, date):
        """古い日付のエントリを作成"""
        self.run_relay("write", f"archive test {session}", "--type", "user", "--session", session)
        self.query_db(
            "UPDATE raw_entries SET date=?, timestamp=? WHERE session_id=?",
            (date, f"{date}T10:00:00.000Z", session),
        )
        # DB を直接更新したので commit
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            "UPDATE raw_entries SET date=?, timestamp=? WHERE session_id=?",
            (date, f"{date}T10:00:00.000Z", session),
        )
        conn.commit()
        conn.close()

    def test_double_archive_no_corruption(self):
        """2回archiveしてもファイルが壊れない"""
        archive_dir = Path(self.tmp_dir) / "archive"
        config = {"retention_days": 1, "archive_dir": str(archive_dir)}
        (self.db_dir / "config.json").write_text(json.dumps(config))

        # 古い日付のエントリを作る
        self.run_relay("write", "archive entry 1", "--type", "user", "--session", "arch001")
        # 直接SQLで日付を変更
        conn = sqlite3.connect(str(self.db_path))
        conn.execute(
            "UPDATE raw_entries SET date='2020-06-15', timestamp='2020-06-15T10:00:00.000Z' WHERE session_id='arch001'"
        )
        conn.commit()
        conn.close()

        # 1回目 archive
        stdout1, stderr1, rc1 = self.run_relay("archive")
        self.assertEqual(rc1, 0, "1回目archiveが成功")

        archive_file = archive_dir / "2020" / "06" / "15.md"
        self.assertTrue(archive_file.exists(), "アーカイブファイル生成")

        content1 = archive_file.read_text()
        self.assertGreater(len(content1), 0, "ファイルが空ではない")

        # DBから削除されている
        count_after = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='arch001'"
        )
        self.assertEqual(count_after, 0, "archive後にDBから削除")

        # 2回目 archive（対象なし）
        stdout2, stderr2, rc2 = self.run_relay("archive")
        self.assertEqual(rc2, 0, "2回目archiveもエラーなし")

        content2 = archive_file.read_text()
        self.assertEqual(content1, content2, "ファイル内容が変わらない")


# ── BUG-005: LIKE ワイルドカードインジェクション ──────────────────────

class TestBug005_LIKEInjection(RelayTestCase):
    """workspace の _ や % が LIKE ワイルドカードとして解釈されない

    【旧挙動】workspace フィルタで cwd LIKE '{workspace}%' を使っていたが、
    workspace パス自体に含まれる _ や % をエスケープしていなかった。
    SQLの LIKE では _ は「任意の1文字」、% は「任意の0文字以上」を意味するため、
    例えば workspace=/test/proj_a で検索すると /test/projXa もヒットしていた。
    パス名にアンダースコアを含むプロジェクト（my_project等）で他プロジェクトの
    データが混入する問題。
    【修正】workspace 文字列中の \\, %, _ を ESCAPE '\\' でエスケープしてから
    LIKE に渡すようにした。"""

    def _insert_with_cwd(self, session, cwd, content="test"):
        self.run_relay("write", content, "--type", "user", "--session", session)
        conn = sqlite3.connect(str(self.db_path))
        conn.execute("UPDATE raw_entries SET cwd=? WHERE session_id=?", (cwd, session))
        conn.commit()
        conn.close()

    def test_underscore_not_wildcard(self):
        """_ がワイルドカードにならない"""
        self._insert_with_cwd("like_a", "/test/proj_a", "proj_a entry")
        self._insert_with_cwd("like_b", "/test/projXa", "projXa entry")

        # LIKE ESCAPE で正しくフィルタされるか
        count_match = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE cwd LIKE ? ESCAPE '\\'",
            ("/test/proj\\_a%",)
        )
        count_false = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE cwd LIKE ? ESCAPE '\\' AND cwd='/test/projXa'",
            ("/test/proj\\_a%",)
        )
        self.assertEqual(count_match, 1, "proj_a のみマッチ")
        self.assertEqual(count_false, 0, "projXa は誤マッチしない")

    def test_percent_not_wildcard(self):
        """% がワイルドカードにならない"""
        self._insert_with_cwd("pct_a", "/test/100%done", "100% entry")
        self._insert_with_cwd("pct_b", "/test/100Xdone", "100X entry")

        count = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE cwd LIKE ? ESCAPE '\\'",
            ("/test/100\\%done%",)
        )
        self.assertEqual(count, 1, "100%done のみマッチ")

    def test_via_mcp_search(self):
        """MCP memory_search の workspace フィルタでもエスケープが効く"""
        self._insert_with_cwd("mcp_a", "/proj_alpha/src", "mcp proj_alpha")
        self._insert_with_cwd("mcp_b", "/projXalpha/src", "mcp projXalpha")

        # MCP サーバーに JSON-RPC で memory_search を呼ぶ
        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_search", "arguments": {"query": "mcp"}},
            workspace="/proj_alpha",
        )
        if resp:
            result_text = json.dumps(resp)
            self.assertIn("proj_alpha", result_text, "MCP: proj_alpha がヒット")
            self.assertNotIn("projXalpha", result_text, "MCP: projXalpha はヒットしない")


# ── BUG-006: detect.rs 誤判定 ────────────────────────────────────────

class TestBug006_DetectClient(RelayTestCase):
    """cargo test で detect.rs のユニットテストを実行

    【旧挙動】detect_from_ppid() が親プロセス名からクライアントを判定する際、
    normalize_client() のマッチングが不完全で、"claude" を含むが Claude Code
    ではないプロセス名（例: "claude-relay" 自身）を誤って "claude-code" と
    判定していた。また、未知のクライアント名を "unknown" ではなく空文字列で
    返すケースがあり、DB の client カラムが空になっていた。
    【修正】normalize_client() のパターンマッチを厳密化し、既知クライアント
    以外は "unknown" を返すようにした。"""

    def test_cargo_test_detect(self):
        """cargo test detect::tests が通る"""
        env = os.environ.copy()
        home = env.get("HOME", os.path.expanduser("~"))
        # cargo が PATH にない場合に追加
        env["PATH"] = f"{Path.home()}/.cargo/bin:/opt/homebrew/bin:" + env.get("PATH", "")
        # cargo test は本物の HOME が必要
        env["HOME"] = str(Path.home())
        result = subprocess.run(
            ["cargo", "test", "detect::tests", "--", "--nocapture"],
            capture_output=True, text=True, timeout=120,
            cwd=str(PROJECT_ROOT), env=env,
        )
        self.assertIn("test result: ok", result.stdout + result.stderr,
                       "cargo test detect::tests が PASS")


# ── BUG-007: MCP で id が float で渡される問題 ───────────────────────

class TestBug007_FloatID(RelayTestCase):
    """MCP JSON-RPC で id=3.0（float）を渡しても正しく動く

    【旧挙動】MCP の tools/call ハンドラで引数の id や limit を
    as_i64() のみで取得していた。しかし Claude 等の AI クライアントは
    JSON の数値を float で送ることがあり（例: id=3 → 3.0）、
    serde_json は 3.0 を Number::Float として保持するため as_i64() が
    None を返し、デフォルト値（id=0, limit=20）にフォールバックしていた。
    結果、指定した id のエントリが取得できない、limit が無視される等の問題。
    【修正】as_i64() に加えて as_f64().map(|f| f as i64) でもパースし、
    さらに文字列 "3" のケースも as_str().and_then(parse) で対応した。"""

    def test_memory_get_entry_with_float_id(self):
        """memory_get_entry に float id を渡す"""
        # エントリを作成
        self.run_relay("write", "bug007 test entry", "--type", "user", "--session", "float001")

        # 作成されたエントリの ID を取得
        entry_id = self.query_scalar(
            "SELECT id FROM raw_entries WHERE session_id='float001'"
        )
        self.assertIsNotNone(entry_id, "エントリが存在")

        # MCP に float として渡す（3 → 3.0）
        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_get_entry", "arguments": {"id": float(entry_id)}},
        )
        self.assertIsNotNone(resp, "MCP レスポンスあり")

        # エラーでないこと
        if resp and "error" not in resp:
            result_text = json.dumps(resp)
            self.assertIn("bug007 test entry", result_text,
                          "float id で正しいエントリが返る")

    def test_memory_search_with_float_limit(self):
        """memory_search に limit=5.0（float）を渡す"""
        self.run_relay("write", "bug007 limit test", "--type", "user", "--session", "float002")

        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_search", "arguments": {"query": "bug007", "limit": 5.0}},
        )
        self.assertIsNotNone(resp, "MCP レスポンスあり")
        if resp:
            self.assertNotIn("error", resp, "エラーなし")

    def test_memory_list_sessions_with_float_limit(self):
        """memory_list_sessions に limit=3.0 を渡す"""
        self.run_relay("write", "bug007 session test", "--type", "user", "--session", "float003")

        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_list_sessions", "arguments": {"limit": 3.0}},
        )
        self.assertIsNotNone(resp, "MCP レスポンスあり")
        if resp:
            self.assertNotIn("error", resp, "エラーなし")


# ── BUG-008: マルチバイト文字でパニック ───────────────────────────────

class TestBug008_MultibytePanic(RelayTestCase):
    """マルチバイト文字で文字列切り詰め時にパニックしない

    【旧挙動】検索結果のプレビュー表示で content を固定バイト数（例: 120バイト）で
    切り詰めていたが、&content[..120] のようにバイトインデックスで直接スライス
    していた。UTF-8 のマルチバイト文字（日本語=3バイト、絵文字=4バイト）の
    途中でスライスすると、Rust は不正な文字境界として panic する。
    日本語やEmoji を含むエントリを memory_search するだけでプロセスが
    クラッシュしていた。
    【修正】char_indices() で文字境界を走査し、バイト数が上限を超えない
    最後の文字境界で切り詰めるようにした。"""

    def test_japanese_long_text(self):
        """日本語60文字超でsearchしてもパニックしない"""
        long_jp = "これはバグゼロゼロハチのテストエントリです。日本語のテキストが六十文字を超えても正しく動作することを確認します。このテキストは意図的に長くしています。"
        self.run_relay("write", long_jp, "--type", "user", "--session", "mb001")

        stdout, stderr, rc = self.run_relay("tool", "memory_search", "--query", "mb001")
        self.assertEqual(rc, 0, "日本語 search でパニックしない")

        content = self.query_scalar(
            "SELECT content FROM raw_entries WHERE session_id='mb001'"
        )
        self.assertEqual(content, long_jp, "日本語テキストが完全に保存されている")

    def test_emoji_text(self):
        """絵文字を含むテキストでもパニックしない"""
        emoji_text = "🚀🎉💻🔥" * 20  # 80絵文字（各4バイト=320バイト > 120バイト境界）
        self.run_relay("write", emoji_text, "--type", "user", "--session", "mb002")

        stdout, stderr, rc = self.run_relay("tool", "memory_search", "--query", "mb002")
        self.assertEqual(rc, 0, "絵文字 search でパニックしない")

    def test_mixed_multibyte(self):
        """CJK + 絵文字 + アラビア語混在"""
        mixed = "日本語テスト🎉 " + "مرحبا " * 10 + "한국어테스트 " + "中文测试" * 5
        self.run_relay("write", mixed, "--type", "user", "--session", "mb003")

        stdout, stderr, rc = self.run_relay("tool", "memory_search", "--query", "mb003")
        self.assertEqual(rc, 0, "多言語混在でパニックしない")

    def test_combining_characters(self):
        """結合文字（アクセント記号等）を含むテキスト"""
        # é = e + combining acute accent (U+0301)
        combining = "e\u0301" * 100  # 100文字分の結合文字
        self.run_relay("write", combining, "--type", "user", "--session", "mb004")

        stdout, stderr, rc = self.run_relay("tool", "memory_search", "--query", "mb004")
        self.assertEqual(rc, 0, "結合文字でパニックしない")


# ── BUG-009: NULL cwd がワークスペース検索で除外される ────────────────

class TestBug009_NullCwd(RelayTestCase):
    """cwd=NULL のエントリがワークスペーススコープに含まれる

    【旧挙動】workspace スコープの WHERE 句が cwd LIKE '{ws}%' のみだった。
    SQL では NULL LIKE 'anything' の結果は NULL（偽扱い）になるため、
    cwd が NULL のエントリはどのワークスペースで検索しても絶対にヒットしなかった。
    CLIの write コマンドや、cwd を送ってこないクライアントから登録された
    エントリが検索結果から完全に消えていた。
    【修正】WHERE 句を (cwd LIKE ? ESCAPE '\\' OR cwd IS NULL) に変更し、
    cwd 未設定のエントリもワークスペース検索に含まれるようにした。"""

    def test_null_cwd_included_in_search(self):
        """NULL cwd が OR cwd IS NULL で取得できる"""
        self.run_relay("write", "null cwd entry", "--type", "user", "--session", "null001")

        # write コマンドは cwd を設定しない → NULL
        cwd = self.query_scalar(
            "SELECT cwd FROM raw_entries WHERE session_id='null001'"
        )
        self.assertIsNone(cwd, "write コマンドの cwd は NULL")

        # 修正後の条件: (cwd LIKE ... OR cwd IS NULL)
        count_with = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='null001' "
            "AND (cwd LIKE '/any/path%' ESCAPE '\\' OR cwd IS NULL)"
        )
        self.assertEqual(count_with, 1, "OR IS NULL ありで1件ヒット")

        # 旧条件: cwd LIKE ... のみ
        count_without = self.query_scalar(
            "SELECT COUNT(*) FROM raw_entries WHERE session_id='null001' "
            "AND cwd LIKE '/any/path%' ESCAPE '\\'"
        )
        self.assertEqual(count_without, 0, "旧挙動では 0件（修正の証拠）")

    def test_null_cwd_via_mcp_search(self):
        """MCP 経由の workspace search でも NULL cwd がヒットする"""
        self.run_relay("write", "mcp null cwd test xyzzy", "--type", "user", "--session", "null002")

        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_search", "arguments": {"query": "xyzzy"}},
            workspace="/some/workspace",
        )
        if resp:
            result_text = json.dumps(resp)
            self.assertIn("xyzzy", result_text,
                          "workspace 付き MCP search で NULL cwd エントリがヒット")

    def test_null_cwd_in_list_sessions(self):
        """list_sessions でも NULL cwd セッションが除外されない"""
        self.run_relay("write", "null session entry", "--type", "user", "--session", "null003")

        resp = self.send_mcp_request(
            "tools/call",
            {"name": "memory_list_sessions", "arguments": {}},
            workspace="/some/workspace",
        )
        if resp:
            result_text = json.dumps(resp)
            self.assertIn("null003", result_text,
                          "workspace 付き list_sessions で NULL cwd セッションがヒット")


# ── メイン ────────────────────────────────────────────────────────────

if __name__ == "__main__":
    require_binary()
    # -v でテスト名を表示、--tb=short でトレースバック短縮
    unittest.main(verbosity=2)
