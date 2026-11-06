"""Desktop-side helper of e2e.sh (stdlib only, run with `python3 -I`).

Every call names the run's temporary folder (`--root`) and refuses an
endpoint file that is not under it: the isolated instance only, never the
person's Hangar.

  control  --root R --endpoint F <op> <json>      POST /api/control/<op>
  wait-pane --root R --endpoint F <ws> <pane> <needle> <secs>
                                                  pane_read until <needle> shows
  pane-env --root R <app pid> <pane id>           HANGAR_* of that pane's shell
  app-runs <app pid> <VAR=value>                  exits 0 if a process under the
                                                  app has it in its environment
  mcp      --root R --endpoint F --app BIN <envfile> <tool> <json> [secs]
                                                  one tool call as that pane
  get      <path>                                 a field of the JSON on stdin
"""

import json
import os
import selectors
import subprocess
import sys
import time
import urllib.error
import urllib.request


def die(message):
    print(f"e2e_helper: {message}", file=sys.stderr)
    sys.exit(1)


def inside(root, path):
    root = os.path.realpath(root)
    path = os.path.realpath(path)
    return path == root or path.startswith(root + os.sep)


def options(args, names):
    """--name value pairs at the front of args, then the rest."""
    found = {}
    while args and args[0].startswith("--") and args[0][2:] in names:
        if len(args) < 2:
            die(f"{args[0]} needs a value")
        found[args[0][2:]] = args[1]
        args = args[2:]
    missing = [name for name in names if name not in found]
    if missing:
        die("missing --" + ", --".join(missing))
    return found, args


def endpoint_of(root, path):
    if not inside(root, path):
        die(f"refusing endpoint {path}: not under the test's folder {root}")
    with open(path, encoding="utf-8") as handle:
        return json.load(handle)


def control(root, endpoint, op, body, timeout=90):
    """The answer of one control op, as (ok, value or error text)."""
    ep = endpoint_of(root, endpoint)
    request = urllib.request.Request(
        f"http://127.0.0.1:{ep['port']}/api/control/{op}",
        data=json.dumps(body).encode(),
        headers={
            "Authorization": f"Bearer {ep['token']}",
            "Content-Type": "application/json",
        },
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as answer:
            text = answer.read().decode()
    except urllib.error.HTTPError as err:
        return False, err.read().decode(errors="replace")
    except (urllib.error.URLError, OSError) as err:
        return False, str(err)
    try:
        value = json.loads(text)
    except ValueError:
        return True, text
    if isinstance(value, dict) and value.get("ok") is False:
        return False, value.get("error") or text
    if isinstance(value, dict) and "value" in value and value.get("ok") is True:
        return True, value["value"]
    return True, value


def cmd_control(args):
    opts, rest = options(args, ["root", "endpoint"])
    if len(rest) != 2:
        die("control <op> <json>")
    ok, value = control(opts["root"], opts["endpoint"], rest[0], json.loads(rest[1]))
    print(value if isinstance(value, str) else json.dumps(value))
    sys.exit(0 if ok else 1)


def cmd_wait_pane(args):
    opts, rest = options(args, ["root", "endpoint"])
    if len(rest) != 4:
        die("wait-pane <workspace> <pane> <needle> <secs>")
    workspace, pane, needle, secs = rest
    deadline = time.time() + float(secs)
    last = ""
    while time.time() < deadline:
        ok, value = control(
            opts["root"],
            opts["endpoint"],
            "pane_read",
            {"workspace": workspace, "pane": pane, "lines": 200},
            timeout=30,
        )
        last = value.get("text", "") if ok and isinstance(value, dict) else str(value)
        if ok and needle in last:
            print(last)
            return
        time.sleep(1)
    print(last)
    die(f"{needle!r} did not show in {pane} within {secs} s")


def environ(pid):
    try:
        with open(f"/proc/{pid}/environ", "rb") as handle:
            raw = handle.read()
    except OSError:
        return {}
    pairs = (item.split(b"=", 1) for item in raw.split(b"\0") if b"=" in item)
    return {key.decode(errors="replace"): value.decode(errors="replace") for key, value in pairs}


def parent(pid):
    try:
        with open(f"/proc/{pid}/stat", encoding="utf-8") as handle:
            stat = handle.read()
    except OSError:
        return 0
    return int(stat[stat.rfind(")") + 2 :].split()[1])


def descends_from(pid, ancestor):
    seen = 0
    while pid > 1 and seen < 64:
        if pid == ancestor:
            return True
        pid = parent(pid)
        seen += 1
    return False


def pane_process(app, want):
    """The oldest process under the app whose environment has VAR=value."""
    key, _, value = want.partition("=")
    for entry in sorted(os.listdir("/proc"), key=lambda n: (len(n), n)):
        if entry.isdigit():
            pid = int(entry)
            env = environ(pid)
            if env.get(key) == value and descends_from(pid, app):
                return pid, env
    return None, {}


def cmd_pane_env(args):
    opts, rest = options(args, ["root"])
    if len(rest) != 2:
        die("pane-env <app pid> <pane id>")
    app, pane = int(rest[0]), rest[1]
    pid, env = pane_process(app, f"HANGAR_PANE_ID={pane}")
    if pid is None:
        die(f"no process of pane {pane} under the app (pid {app})")
    hangar = {k: v for k, v in env.items() if k.startswith("HANGAR_")}
    if not hangar.get("HANGAR_BRIDGE_URL") or not hangar.get("HANGAR_BRIDGE_TOKEN"):
        die(f"pane {pane} (pid {pid}) has no bridge in its environment")
    # Where the pane runs: the workspace its board calls are for.
    hangar["HANGAR_WORKSPACE"] = os.readlink(f"/proc/{pid}/cwd")
    if not inside(opts["root"], hangar["HANGAR_WORKSPACE"]):
        die(f"pane {pane} runs in {hangar['HANGAR_WORKSPACE']}, not under {opts['root']}")
    for key, value in sorted(hangar.items()):
        print(f"{key}={value}")


def cmd_app_runs(args):
    if len(args) != 2:
        die("app-runs <app pid> <VAR=value>")
    pid, _ = pane_process(int(args[0]), args[1])
    if pid is None:
        sys.exit(1)
    print(pid)


def cmd_mcp(args):
    opts, rest = options(args, ["root", "endpoint", "app"])
    if len(rest) not in (3, 4):
        die("mcp <envfile> <tool> <json> [secs]")
    envfile, tool, arguments = rest[:3]
    secs = float(rest[3]) if len(rest) == 4 else 60.0
    if not inside(opts["root"], opts["endpoint"]) or not inside(opts["root"], envfile):
        die("refusing: the endpoint and the pane's environment must be the test's own")
    env = {k: v for k, v in os.environ.items() if not k.startswith("HANGAR_")}
    with open(envfile, encoding="utf-8") as handle:
        for line in handle:
            key, _, value = line.rstrip("\n").partition("=")
            if key.startswith("HANGAR_"):
                env[key] = value
    env["HANGAR_ENDPOINT_FILE"] = opts["endpoint"]
    # The proxy takes its workspace from HANGAR_WORKSPACE, else its folder: both
    # are the pane's own, inside the test's folder, never the caller's checkout.
    workspace = env.get("HANGAR_WORKSPACE", "")
    if not workspace or not inside(opts["root"], workspace):
        die(f"refusing: the pane's workspace {workspace!r} is not under {opts['root']}")
    calls = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": {"name": "hangar-node-e2e", "version": "1"}}},
        {"jsonrpc": "2.0", "method": "notifications/initialized"},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": tool, "arguments": json.loads(arguments)}},
    ]
    proxy = subprocess.Popen(
        [opts["app"], "--mcp"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        env=env,
        cwd=workspace,
        text=True,
    )
    try:
        for call in calls:
            proxy.stdin.write(json.dumps(call) + "\n")
        proxy.stdin.flush()
        deadline = time.time() + secs
        answer = None
        selector = selectors.DefaultSelector()
        selector.register(proxy.stdout, selectors.EVENT_READ)
        while time.time() < deadline and answer is None:
            if not selector.select(timeout=max(0.1, deadline - time.time())):
                break
            line = proxy.stdout.readline()
            if not line:
                break
            try:
                message = json.loads(line)
            except ValueError:
                continue
            if message.get("id") == 2:
                answer = message
    finally:
        proxy.kill()
        proxy.wait()
    if answer is None:
        die(f"{tool}: no answer within {secs} s")
    if "error" in answer:
        print(json.dumps(answer["error"]))
        sys.exit(1)
    result = answer.get("result", {})
    text = "\n".join(
        part.get("text", "") for part in result.get("content", []) if isinstance(part, dict)
    )
    print(text)
    sys.exit(1 if result.get("isError") else 0)


def cmd_get(args):
    if len(args) != 1:
        die("get <path>")
    value = json.load(sys.stdin)
    for key in filter(None, args[0].split(".")):
        if isinstance(value, list):
            value = value[int(key)] if key.lstrip("-").isdigit() and -len(value) <= int(key) < len(value) else None
        elif isinstance(value, dict):
            value = value.get(key)
        else:
            value = None
    print(value if isinstance(value, str) else json.dumps(value))


COMMANDS = {
    "control": cmd_control,
    "wait-pane": cmd_wait_pane,
    "pane-env": cmd_pane_env,
    "app-runs": cmd_app_runs,
    "mcp": cmd_mcp,
    "get": cmd_get,
}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        die("usage: " + " | ".join(COMMANDS))
    COMMANDS[sys.argv[1]](sys.argv[2:])
