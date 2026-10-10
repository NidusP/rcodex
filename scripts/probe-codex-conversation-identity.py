#!/usr/bin/env python3
"""Verify Codex HTTP conversation headers using disposable homes and a loopback mock.

No real user auth/history is read. Only the request identity envelope and the
first native session_meta record are retained in the printed report. All mock
content and temporary Codex files are deleted after the run. No model tools run.
"""
import argparse
from collections import Counter
import concurrent.futures
import http.server
import json
import os
from pathlib import Path
import subprocess
import tempfile
import threading

parser = argparse.ArgumentParser()
parser.add_argument("--codex", default="/Applications/ChatGPT.app/Contents/Resources/codex")
args = parser.parse_args()
observations = []
lock = threading.Lock()

class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", "0"))))
        headers = {key.lower(): value for key, value in self.headers.items()}
        with lock:
            observations.append({"headers": {key: value for key, value in headers.items()
                if key in {"session_id", "thread_id", "session-id", "thread-id", "x-codex-turn-metadata", "x-client-request-id"}},
                "bodyIdentity": {key: body[key] for key in {"session_id", "thread_id", "conversation_id", "turn_id", "metadata"} if key in body}})
        item = {"id": "msg_probe", "type": "message", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": "Local identity probe complete.", "annotations": []}]}
        response = {"id": "resp_probe", "object": "response", "status": "completed", "output": [item], "usage": {"input_tokens": 1, "output_tokens": 1, "total_tokens": 2}}
        payload = "".join("data: " + json.dumps(event) + "\n\n" for event in [
            {"type": "response.output_item.added", "output_index": 0, "item": item},
            {"type": "response.output_item.done", "output_index": 0, "item": item},
            {"type": "response.completed", "response": response}]).encode()
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.send_header("content-length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)
    def log_message(self, *_):
        pass

server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
try:
    with tempfile.TemporaryDirectory(prefix="rcodex-identity-") as temporary:
        root = Path(temporary)
        home = root / "codex-home"
        project = root / "project"
        home.mkdir()
        project.mkdir()
        (home / "config.toml").write_text(f'''model = "gpt-5.2-codex"
model_provider = "identity_probe"
web_search = "disabled"
[analytics]
enabled = false
[features]
plugins = false
apps = false
[model_providers.identity_probe]
name = "Local identity probe"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
request_max_retries = 0
stream_max_retries = 0
stream_idle_timeout_ms = 5000
''')
        # Give the child a fresh home and a strict environment allowlist. Proxy
        # remote optional discovery to a closed loopback port; model traffic
        # alone bypasses that proxy to reach this disposable mock.
        env = {key: value for key, value in os.environ.items() if key in {"PATH", "TMPDIR", "SYSTEMROOT", "WINDIR"}}
        env.update({"HOME": str(root), "CODEX_HOME": str(home), "CODEX_SQLITE_HOME": str(home), "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost", "HTTP_PROXY": "http://127.0.0.1:9", "HTTPS_PROXY": "http://127.0.0.1:9", "http_proxy": "http://127.0.0.1:9", "https_proxy": "http://127.0.0.1:9"})
        def run(resume=None):
            command = [args.codex, "exec", "--skip-git-repo-check", "--json", "--ignore-rules"]
            if resume:
                command += ["resume", resume, "This is a disposable local mock probe; reply without tools."]
            else:
                command += ["--sandbox", "read-only", "This is a disposable local mock probe; reply without tools."]
            result = subprocess.run(command, cwd=project, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=35)
            events = [json.loads(line) for line in result.stdout.splitlines() if line.startswith("{")]
            started = next((item["thread_id"] for item in events if item.get("type") == "thread.started"), None)
            if result.returncode or not started:
                raise RuntimeError(f"probe failed: {result.returncode} {result.stderr[-1500:]} {result.stdout[-1500:]}")
            return started
        version = subprocess.run([args.codex, "--version"], cwd=project, env=env, stdin=subprocess.DEVNULL, capture_output=True, text=True, check=True).stdout.strip()
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            new_ids = list(pool.map(lambda _: run(), range(2)))
        resumed = run(new_ids[0])
        native = {}
        for path in home.glob("sessions/**/*.jsonl"):
            with path.open("rb") as handle:
                first = handle.readline(256 * 1024)
            record = json.loads(first)
            assert record["type"] == "session_meta"
            native[record["payload"]["id"]] = str(path.relative_to(home))
        assert len(set(new_ids)) == 2 and resumed == new_ids[0]
        assert set(native) == set(new_ids)
        assert len(observations) == 3
        observed_conversations = []
        observed_turns = set()
        for observation in observations:
            values = observation["headers"]
            candidate = values.get("session_id") or values.get("session-id")
            assert candidate in native, observation
            if "thread_id" in values:
                assert values["thread_id"] == candidate, observation
            if "thread-id" in values:
                assert values["thread-id"] == candidate, observation
            metadata = json.loads(values["x-codex-turn-metadata"])
            assert metadata["session_id"] == metadata["thread_id"] == candidate
            observed_conversations.append(candidate)
            observed_turns.add(metadata["turn_id"])
        assert Counter(observed_conversations) == Counter([*new_ids, resumed])
        assert len(observed_turns) == 3
        print(json.dumps({"version": version, "newConversationIds": new_ids, "resumedConversationId": resumed, "nativeRollouts": native, "requests": observations, "verified": True}, ensure_ascii=False, indent=2))
finally:
    server.shutdown()
    server.server_close()
