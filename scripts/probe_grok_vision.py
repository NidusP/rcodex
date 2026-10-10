#!/usr/bin/env python3
from __future__ import annotations
import base64, json, sqlite3, subprocess, sys, urllib.error, urllib.request
from pathlib import Path
from urllib.parse import urlparse
REPO = Path(__file__).resolve().parents[1]
FIXTURE = REPO / "src-tauri/tests/fixtures/grok-vision-probe-red-square.png"
DB = Path.home() / "Library/Application Support/io.rcodex/rcodex.sqlite3"
MODEL = "grok-4.6"
PROVIDER_NAME = "3rxai"

def die(msg, code=1):
    print("Error: " + msg, file=sys.stderr)
    raise SystemExit(code)

def keychain_password(account):
    result = subprocess.run(["security","find-generic-password","-s","io.rcodex","-a",account,"-w"], capture_output=True, text=True, timeout=8, check=False)
    if result.returncode != 0:
        return None
    return result.stdout.strip() or None

def load_3rxai():
    if not DB.exists():
        die("rcodex sqlite not found")
    con = sqlite3.connect("file:%s?mode=ro" % DB, uri=True)
    con.row_factory = sqlite3.Row
    row = con.execute("SELECT p.name, coalesce(r.base_url, p.base_url) AS base_url, r.secret_ref AS rev_ref, p.secret_ref AS prov_ref FROM api_key_providers p LEFT JOIN api_key_provider_revisions r ON r.id = p.current_revision_id WHERE p.name = ?", (PROVIDER_NAME,)).fetchone()
    con.close()
    if row is None:
        die("provider 3rxai not found")
    base = (row["base_url"] or "").rstrip("/")
    if not base:
        die("3rxai base_url empty")
    if urlparse(base).hostname in {"127.0.0.1","localhost","::1"}:
        die("refusing loopback gateway for vision probe")
    secret_ref = (row["rev_ref"] or row["prov_ref"] or "").strip()
    key = keychain_password(secret_ref) if secret_ref else None
    if not key:
        die("could not read 3rxai key from keychain")
    return base, key

def main():
    if not FIXTURE.exists():
        die("missing fixture")
    png = FIXTURE.read_bytes()
    data_url = "data:image/png;base64," + base64.b64encode(png).decode("ascii")
    base, key = load_3rxai()
    url = base + "/responses"
    body = {"model": MODEL, "stream": False, "reasoning": {"effort": "low"}, "input": [{"role": "user", "content": [{"type": "input_text", "text": "Look at the image. Reply with exactly two lowercase English words: the main color, then the shape. Example: blue circle. No other text."}, {"type": "input_image", "detail": "low", "image_url": data_url}]}]}
    req = urllib.request.Request(url, data=json.dumps(body).encode("utf-8"), method="POST", headers={"Authorization": "Bearer " + key, "Content-Type": "application/json"})
    print("probe_target=3rxai")
    print("probe_url_host=" + (urlparse(url).hostname or ""))
    print("probe_path=/v1/responses")
    print("probe_model=" + MODEL)
    print("probe_bytes=" + str(len(png)))
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            raw = resp.read()
            status = resp.status
    except urllib.error.HTTPError as err:
        raw = err.read()
        status = err.code
        print("http_status=" + str(status))
        print("upstream_error_excerpt=" + json.dumps(raw[:800].decode("utf-8", errors="replace"), ensure_ascii=False))
        print("probe_result=fail")
        raise SystemExit(2)
    print("http_status=" + str(status))
    try:
        payload = json.loads(raw)
    except json.JSONDecodeError:
        print("body_not_json=1")
        print("probe_result=fail")
        raise SystemExit(3)
    usage = payload.get("usage") or {}
    print("input_tokens=" + str(usage.get("input_tokens")))
    print("output_tokens=" + str(usage.get("output_tokens")))
    texts = []
    def walk(value):
        if isinstance(value, dict):
            if value.get("type") in {"output_text","text"} and isinstance(value.get("text"), str):
                texts.append(value["text"])
            for nested in value.values():
                walk(nested)
        elif isinstance(value, list):
            for nested in value:
                walk(nested)
    walk(payload.get("output") or payload)
    reply = " ".join(part.strip() for part in texts if part and part.strip())
    print("output_status=" + json.dumps(payload.get("status")))
    print("reply=" + json.dumps(reply[:500], ensure_ascii=False))
    lowered = reply.lower()
    color_ok = any(token in lowered for token in ("red","hong","scarlet","crimson")) or ("红" in reply)
    shape_ok = any(token in lowered for token in ("square","rectangle","block")) or ("方" in reply)
    print("recognized_red=" + str(color_ok).lower())
    print("recognized_square=" + str(shape_ok).lower())
    if 200 <= status < 300 and (color_ok or shape_ok):
        print("probe_result=pass")
        return
    print("probe_result=fail")
    raise SystemExit(4)

if __name__ == "__main__":
    main()
