#!/usr/bin/env python3
"""Set up (or tear down) the owner's Cloudflare side of the assisted connection.

  scripts/cloudflare-setup.py setup    [--name NAME] [--subdomain SUB] [--out FILE]
  scripts/cloudflare-setup.py teardown [--name NAME] [--out FILE]

Creates a Realtime TURN key and deploys infra/owner-relay (Worker + SQLite Durable
Object) to YOUR account. The account API token is read from stdin (or a hidden prompt),
used for this run only, never written to disk or printed. Only the TURN key id and its
credential token go to --out (mode 0600). Re-running is safe. Stdlib only.

Token permissions (Account level): Workers Scripts: Edit, Account Settings: Read,
and the one covering Realtime/Calls TURN keys (edit). The script names the call that
failed if one is missing.
"""
import argparse, getpass, json, os, secrets, sys, urllib.error, urllib.request
from pathlib import Path

API = os.environ.get("CF_API", "https://api.cloudflare.com/client/v4")
DIST = Path(__file__).resolve().parent.parent / "infra" / "owner-relay" / "dist"
TOKEN = ""


class Fail(Exception):
    pass


def call(method, path, body=None, raw=None, ctype="application/json", what=""):
    data = raw if raw is not None else (json.dumps(body).encode() if body is not None else None)
    req = urllib.request.Request(API + path, data=data, method=method)
    req.add_header("Authorization", "Bearer " + TOKEN)
    if data is not None:
        req.add_header("Content-Type", ctype)
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            out = json.loads(r.read() or b"{}")
    except urllib.error.HTTPError as e:
        try:
            out = json.loads(e.read())
        except Exception:
            out = {}
        if e.code == 404 and method == "GET":
            return None
        msgs = "; ".join(str(x.get("message")) for x in out.get("errors", [])) or e.reason
        hint = " (token lacks a permission for this call)" if e.code in (401, 403) else ""
        raise Fail(f"{what or method + ' ' + path}: HTTP {e.code}: {msgs}{hint}")
    except urllib.error.URLError as e:
        raise Fail(f"{what or path}: cannot reach Cloudflare: {e.reason}")
    return out.get("result", out)


def read_token():
    if sys.stdin.isatty():
        return getpass.getpass("Cloudflare account API token (hidden): ").strip()
    return sys.stdin.readline().strip()


def account(want=None):
    accs = call("GET", "/accounts", what="list accounts (needs Account Settings: Read)")
    if not accs:
        raise Fail("the token sees no account")
    if want:
        accs = [x for x in accs if want in (x["id"], x["name"])]
        if not accs:
            raise Fail(f"no account named or numbered {want!r} is visible to the token")
    if len(accs) > 1:
        names = ", ".join(a["name"] for a in accs)
        raise Fail(f"the token sees several accounts ({names}); pick one with --account NAME")
    return accs[0]["id"]


def multipart(parts):
    b = "----tb" + secrets.token_hex(8)
    out = b""
    for name, fname, ctype, content in parts:
        out += f'--{b}\r\nContent-Disposition: form-data; name="{name}"; filename="{fname}"\r\nContent-Type: {ctype}\r\n\r\n'.encode()
        out += content + b"\r\n"
    return out + f"--{b}--\r\n".encode(), f"multipart/form-data; boundary={b}"


def load_out(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return {}


def save_out(path, data):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w") as f:
        json.dump(data, f, indent=2)
    os.chmod(path, 0o600)


def setup(a, acc):
    state = load_out(a.out)
    # 1. TURN key (kept across re-runs; the key secret is only shown at creation)
    if state.get("turn_key_id") and state.get("turn_key_token"):
        print(f"TURN key: already in {a.out}, kept")
    else:
        k = call("POST", f"/accounts/{acc}/calls/turn_keys", {"name": a.name},
                 what="create TURN key (needs the Realtime/Calls edit permission)")
        state.update(turn_key_id=k["uid"], turn_key_token=k["secret"])
        save_out(a.out, state)
        print(f"TURN key: created ({k['uid']})")
    # 2. workers.dev subdomain
    sub = call("GET", f"/accounts/{acc}/workers/subdomain", what="read workers.dev subdomain")
    sub = (sub or {}).get("subdomain")
    if not sub:
        if not a.subdomain:
            raise Fail("this account has no workers.dev subdomain; rerun with --subdomain NAME to create one")
        call("PUT", f"/accounts/{acc}/workers/subdomain", {"subdomain": a.subdomain}, what="create workers.dev subdomain")
        sub = a.subdomain
    # 3. Worker + Durable Object
    migs = json.loads((DIST / "migrations.json").read_text())
    meta = {
        "main_module": "worker.js",
        "compatibility_date": "2024-09-23",
        "bindings": [{"type": "durable_object_namespace", "name": "RELAY", "class_name": "Relay"}],
        "migrations": {"new_tag": migs[0]["tag"], "new_sqlite_classes": migs[0]["new_sqlite_classes"]},
    }
    body, ctype = multipart([
        ("metadata", "metadata.json", "application/json", json.dumps(meta).encode()),
        ("worker.js", "worker.js", "application/javascript+module", (DIST / "worker.js").read_bytes()),
    ])
    r = call("PUT", f"/accounts/{acc}/workers/scripts/{a.name}", raw=body, ctype=ctype,
             what="upload Worker (needs Workers Scripts: Edit; a free plan must allow SQLite Durable Objects)")
    call("POST", f"/accounts/{acc}/workers/scripts/{a.name}/subdomain", {"enabled": True}, what="enable workers.dev route")
    url = f"wss://{a.name}.{sub}.workers.dev"
    state.update(worker_name=a.name, worker_url=url)
    save_out(a.out, state)
    print(f"Worker:   deployed ({a.name}); durable objects migrated: {bool(r)}")
    print(f"\nSignaling URL: {url}")
    print(f"Saved to {a.out} (0600): TURN key id and credential token only. The account token was not stored.")
    print("Free allowance: 1,000 GB/month of TURN egress. Undo everything with: cloudflare-setup.py teardown")


def teardown(a, acc):
    state = load_out(a.out)
    name = state.get("worker_name", a.name)
    call("DELETE", f"/accounts/{acc}/workers/scripts/{name}?force=true", what="delete Worker")
    print(f"Worker:   {name} deleted (or was absent)")
    if state.get("turn_key_id"):
        call("DELETE", f"/accounts/{acc}/calls/turn_keys/{state['turn_key_id']}", what="delete TURN key")
        print(f"TURN key: {state['turn_key_id']} deleted")
    if Path(a.out).exists():
        Path(a.out).unlink()
        print(f"Removed {a.out}")


def main():
    global TOKEN
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("action", choices=["setup", "teardown"])
    p.add_argument("--name", default="termenbox-relay-test")
    p.add_argument("--account", help="account name or id, when the token sees several")
    p.add_argument("--subdomain")
    p.add_argument("--out", default="cloudflare-relay.json")
    a = p.parse_args()
    TOKEN = read_token()
    if not TOKEN:
        sys.exit("no token given")
    try:
        acc = account(a.account)
        (setup if a.action == "setup" else teardown)(a, acc)
    except Fail as e:
        sys.exit(f"error: {e}")
    finally:
        TOKEN = ""


if __name__ == "__main__":
    main()
