"""Measuring side of bench.sh (stdlib only, run with `python3 -I`).

Every desktop call names the run's temporary folder (`--root`) and refuses an
endpoint file that is not under it: the isolated instance only, never the
person's Hangar. The node is reached through the bench's own SSH tunnel to
its API (`--url`, `--token`: node.json's).

  node-echo   --url U --token T --pane ID [--samples N] [--via ws|post]
                  keystroke -> echo through the node API: a pane of its own,
                  its WebSocket stream, the key sent as an {"input"} frame on
                  it (what the desktop does) or POST /input
  node-flood  --url U --token T --pane ID [--lines N]
                  `seq 1 N` in that pane: seconds until the last line came,
                  stream bytes and frames
  node-spawn  --url U --token T --pane ID     an idle bash pane on the node
  node-kill   --url U --token T --pane ID
  local-echo  [--samples N]                   the same keystrokes into a bash
                                              on a local PTY (the floor)
  desktop-echo  --root R --endpoint F <ws> <pane> [--samples N]
                  pane_send of one key, pane_read polled until it shows
  desktop-flood --root R --endpoint F <ws> <pane> [--lines N]
                  `seq 1 N` sent with pane_send, pane_read polled until the
                  line after it shows
  desktop-prepare --root R --endpoint F <ws> <pane>
                  a plain bash with a fixed prompt in that pane
  connect --root R --endpoint F --since NS --log LOG --name NAME <ws> <pane>
                  an app just started (at NS, epoch nanoseconds): milliseconds
                  until its endpoint file, then until the pane shows its first
                  text (its prompt, or the screen replayed), and the
                  connection's steps from LOG (HANGAR_NODES_TIMING lines)
  report <results.jsonl> <out.json>
                  the median of each number over the runs of each measure

Every measure prints one JSON object.
"""

import base64
import http.client
import json
import os
import pty
import select
import socket
import statistics
import struct
import sys
import time
import urllib.parse

PROMPT = "B> "
KEYS = "abcdefghijklmnopqrstuvwxyz"
# Keys typed on one line before it is cleared (ctrl+c).
LINE = 30


def die(message):
    print(f"bench_helper: {message}", file=sys.stderr)
    sys.exit(1)


def inside(root, path):
    root = os.path.realpath(root)
    path = os.path.realpath(path)
    return path == root or path.startswith(root + os.sep)


def options(args, names, required=()):
    """--name value pairs anywhere in args, then the rest."""
    found, rest = {}, []
    i = 0
    while i < len(args):
        if args[i].startswith("--") and args[i][2:] in names:
            if i + 1 >= len(args):
                die(f"{args[i]} needs a value")
            found[args[i][2:]] = args[i + 1]
            i += 2
        else:
            rest.append(args[i])
            i += 1
    missing = [name for name in required if name not in found]
    if missing:
        die("missing --" + ", --".join(missing))
    return found, rest


def summary(samples_ms):
    ordered = sorted(samples_ms)
    p95 = ordered[min(len(ordered) - 1, int(round(0.95 * (len(ordered) - 1))))]
    return {
        "n": len(ordered),
        "median_ms": round(statistics.median(ordered), 3),
        "p95_ms": round(p95, 3),
        "min_ms": round(ordered[0], 3),
        "max_ms": round(ordered[-1], 3),
    }


# --- The node's API ----------------------------------------------------------


class Node:
    def __init__(self, url, token):
        parsed = urllib.parse.urlparse(url)
        self.host = parsed.hostname
        self.port = parsed.port
        self.token = token
        self.http = None

    def request(self, method, path, body=None):
        for attempt in (1, 2):
            if self.http is None:
                self.http = http.client.HTTPConnection(self.host, self.port, timeout=30)
                self.http.connect()
                self.http.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            headers = {"Authorization": f"Bearer {self.token}"}
            payload = None
            if body is not None:
                payload = json.dumps(body).encode()
                headers["Content-Type"] = "application/json"
            try:
                self.http.request(method, path, body=payload, headers=headers)
                answer = self.http.getresponse()
                text = answer.read().decode(errors="replace")
            except (OSError, http.client.HTTPException):
                self.http.close()
                self.http = None
                if attempt == 2:
                    raise
                continue
            if answer.status >= 300:
                die(f"{method} {path}: HTTP {answer.status} {text}")
            return json.loads(text) if text else None
        return None


class Socket:
    """RFC 6455's client side, enough for a pane's stream."""

    def __init__(self, node, path):
        self.sock = socket.create_connection((node.host, node.port), timeout=30)
        self.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        key = base64.b64encode(os.urandom(16)).decode()
        self.sock.sendall(
            (
                f"GET {path} HTTP/1.1\r\nHost: {node.host}:{node.port}\r\n"
                f"Authorization: Bearer {node.token}\r\nUpgrade: websocket\r\n"
                f"Connection: Upgrade\r\nSec-WebSocket-Key: {key}\r\n"
                # Output in binary frames, from a node that speaks them.
                "Sec-WebSocket-Protocol: hangar-binary\r\n"
                "Sec-WebSocket-Version: 13\r\n\r\n"
            ).encode()
        )
        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(65536)
            if not chunk:
                die("closed during the handshake")
            buf += chunk
        head, self.buf = buf.split(b"\r\n\r\n", 1)
        if b" 101 " not in head.split(b"\r\n", 1)[0]:
            die(f"no upgrade: {head[:200]!r}")
        self.wire_bytes = len(self.buf)
        self.frames = 0

    def send_text(self, text):
        payload = text.encode()
        mask = os.urandom(4)
        head = bytes([0x81])
        n = len(payload)
        if n < 126:
            head += bytes([0x80 | n])
        elif n < 65536:
            head += bytes([0x80 | 126]) + struct.pack(">H", n)
        else:
            head += bytes([0x80 | 127]) + struct.pack(">Q", n)
        masked = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        self.sock.sendall(head + mask + masked)

    def _frame(self):
        """One whole frame off the buffer: (opcode, payload), or None."""
        buf = self.buf
        if len(buf) < 2:
            return None
        opcode = buf[0] & 0x0F
        n = buf[1] & 0x7F
        at = 2
        if n == 126:
            if len(buf) < 4:
                return None
            n = struct.unpack(">H", buf[2:4])[0]
            at = 4
        elif n == 127:
            if len(buf) < 10:
                return None
            n = struct.unpack(">Q", buf[2:10])[0]
            at = 10
        if len(buf) < at + n:
            return None
        payload = buf[at : at + n]
        self.buf = buf[at + n :]
        return opcode, payload

    def recv(self, timeout=30):
        """The next message: a text one as JSON; a binary one (a node that
        speaks them: 8 bytes of seq, big-endian, then the data) as
        {"seq", "data"} too."""
        deadline = time.monotonic() + timeout
        while True:
            frame = self._frame()
            if frame is not None:
                opcode, payload = frame
                if opcode == 0x8:
                    die("the node closed the stream")
                if opcode == 0x1:
                    self.frames += 1
                    return json.loads(payload)
                if opcode == 0x2:
                    self.frames += 1
                    seq = struct.unpack(">Q", payload[:8])[0]
                    return {"seq": seq, "data": payload[8:].decode()}
                continue
            left = deadline - time.monotonic()
            if left <= 0:
                die("nothing on the stream in time")
            self.sock.settimeout(left)
            chunk = self.sock.recv(1 << 20)
            if not chunk:
                die("the stream ended")
            self.wire_bytes += len(chunk)
            self.buf += chunk


class Stream:
    """A pane's stream, its output read as text."""

    def __init__(self, node, pane, start=0):
        self.node = node
        self.pane = pane
        # From what the pane printed so far: an earlier run's keys, replayed,
        # would pass for this run's echoes.
        self.ws = Socket(node, f"/node/panes/{urllib.parse.quote(pane)}/stream?from={start}")
        self.data_bytes = 0
        self.resets = 0
        self.screen = ""

    def read(self, timeout=30):
        message = self.ws.recv(timeout)
        if "exit" in message:
            die("the pane's shell ended")
        if "reset" in message:
            self.resets += 1
        data = message.get("data", "")
        self.data_bytes += len(data.encode())
        self.screen = (self.screen + data)[-4096:]
        return data

    def until(self, needle, timeout=30):
        deadline = time.monotonic() + timeout
        while needle not in self.screen:
            self.read(max(0.1, deadline - time.monotonic()))

    def send(self, data, via="ws"):
        if via == "ws":
            self.ws.send_text(json.dumps({"input": data}))
        else:
            self.node.request("POST", f"/node/panes/{self.pane}/input", {"data": data})


def node_of(opts):
    return Node(opts["url"], opts["token"])


def spawn_body(pane):
    return {
        "id": pane,
        "cwd": "/home/hangar",
        "cols": 120,
        "rows": 40,
        "shell": {"program": "bash", "args": ["--norc", "--noprofile", "-i"]},
    }


def cmd_node_spawn(args):
    opts, _ = options(args, ["url", "token", "pane"], ["url", "token", "pane"])
    node_of(opts).request("POST", "/node/panes", spawn_body(opts["pane"]))
    print(json.dumps({"spawned": opts["pane"]}))


def cmd_node_kill(args):
    opts, _ = options(args, ["url", "token", "pane"], ["url", "token", "pane"])
    node_of(opts).request("DELETE", f"/node/panes/{opts['pane']}")
    print(json.dumps({"killed": opts["pane"]}))


def ready_stream(opts):
    node = node_of(opts)
    pane = opts["pane"]
    listed = node.request("GET", "/node/panes")
    found = [p for p in listed["panes"] if p["id"] == pane and p["alive"]]
    if not found:
        node.request("POST", "/node/panes", spawn_body(pane))
    stream = Stream(node, pane, found[0]["seq"] if found else 0)
    tag = time.monotonic_ns() % 1000000
    stream.send(f"PS1='{PROMPT}'; bind 'set enable-bracketed-paste off'; echo re{tag}$((1+1))ady\r")
    stream.until(f"re{tag}2ady")
    stream.until(PROMPT)
    return stream


def echo_samples(samples, send, wait_for):
    """Types `samples` keys, one at a time, each timed until it is echoed.
    A line is cleared (ctrl+c) every LINE keys."""
    times = []
    typed = 0
    for i in range(samples):
        key = KEYS[i % len(KEYS)]
        start = time.perf_counter()
        send(key)
        wait_for(key)
        times.append((time.perf_counter() - start) * 1000)
        typed += 1
        if typed == LINE or i == samples - 1:
            # Cleared at the end too: what comes next types on a clean line.
            send("\x03")
            wait_for(None)
            typed = 0
    return times


def cmd_node_echo(args):
    opts, _ = options(args, ["url", "token", "pane", "samples", "via"], ["url", "token", "pane"])
    samples = int(opts.get("samples", 200))
    via = opts.get("via", "ws")
    stream = ready_stream(opts)

    def wait_for(key):
        if key is None:
            stream.screen = ""
            stream.until(PROMPT)
            return
        while True:
            if key in stream.read():
                return

    times = echo_samples(samples, lambda k: stream.send(k, via), wait_for)
    print(json.dumps({"measure": f"node-echo-{via}", **summary(times)}))


def cmd_node_flood(args):
    opts, _ = options(args, ["url", "token", "pane", "lines"], ["url", "token", "pane"])
    lines = int(opts.get("lines", 2_000_000))
    stream = ready_stream(opts)
    stream.screen = ""
    stream.data_bytes = 0
    frames0, wire0 = stream.ws.frames, stream.ws.wire_bytes
    start = time.perf_counter()
    stream.send(f"seq 1 {lines}; echo fl$((1+1))od\r")
    stream.until("fl2od", timeout=600)
    seconds = time.perf_counter() - start
    frames = stream.ws.frames - frames0
    wire = stream.ws.wire_bytes - wire0
    print(
        json.dumps(
            {
                "measure": "node-flood",
                "lines": lines,
                "seconds": round(seconds, 3),
                "data_bytes": stream.data_bytes,
                "wire_bytes": wire,
                "frames": frames,
                # The stream fell behind the node's history: output skipped.
                "resets": stream.resets,
                "bytes_per_frame": round(stream.data_bytes / max(frames, 1)),
                "mb_per_s": round(stream.data_bytes / seconds / 1e6, 2),
            }
        )
    )


# --- A local PTY: the floor ---------------------------------------------------


def cmd_local_echo(args):
    opts, _ = options(args, ["samples"])
    samples = int(opts.get("samples", 200))
    pid, fd = pty.fork()
    if pid == 0:
        os.environ.clear()
        os.environ.update({"PATH": "/usr/bin:/bin", "TERM": "xterm-256color", "HOME": "/tmp"})
        os.execv("/bin/bash", ["bash", "--norc", "--noprofile", "-i"])
    screen = [""]

    def read(timeout=10):
        ready, _, _ = select.select([fd], [], [], timeout)
        if not ready:
            die("the local bash said nothing")
        data = os.read(fd, 65536).decode(errors="replace")
        screen[0] = (screen[0] + data)[-4096:]
        return data

    def until(needle):
        while needle not in screen[0]:
            read()

    os.write(fd, f"PS1='{PROMPT}'; bind 'set enable-bracketed-paste off'; echo re$((1+1))ady\r".encode())
    until("re2ady")
    until(PROMPT)

    def wait_for(key):
        if key is None:
            screen[0] = ""
            until(PROMPT)
            return
        while key not in read():
            pass

    times = echo_samples(samples, lambda k: os.write(fd, k.encode()), wait_for)
    os.kill(pid, 9)
    print(json.dumps({"measure": "local-pty-echo", **summary(times)}))


# --- The desktop's control API -------------------------------------------------


class Control:
    def __init__(self, root, endpoint):
        if not inside(root, endpoint):
            die(f"refusing endpoint {endpoint}: not under the bench's folder {root}")
        with open(endpoint, encoding="utf-8") as handle:
            ep = json.load(handle)
        self.port = ep["port"]
        self.token = ep["token"]
        self.http = None

    def call(self, op, body, strict=True):
        for attempt in (1, 2):
            if self.http is None:
                self.http = http.client.HTTPConnection("127.0.0.1", self.port, timeout=60)
                self.http.connect()
                self.http.sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
            try:
                self.http.request(
                    "POST",
                    f"/api/control/{op}",
                    body=json.dumps(body).encode(),
                    headers={
                        "Authorization": f"Bearer {self.token}",
                        "Content-Type": "application/json",
                    },
                )
                answer = self.http.getresponse()
                text = answer.read().decode(errors="replace")
            except (OSError, http.client.HTTPException):
                self.http.close()
                self.http = None
                if attempt == 2:
                    raise
                continue
            value = json.loads(text) if text else None
            if answer.status >= 300 or (isinstance(value, dict) and value.get("ok") is False):
                if not strict:
                    return None
                die(f"{op}: {text}")
            if isinstance(value, dict) and value.get("ok") is True and "value" in value:
                return value["value"]
            return value
        return None


def shown(control, workspace, pane):
    """The pane's text, or None while the app cannot say (no such pane yet,
    or its terminal not up)."""
    try:
        value = control.call("pane_read", {"workspace": workspace, "pane": pane, "lines": 5}, strict=False)
    except (OSError, http.client.HTTPException, ValueError):
        return None
    return value.get("text", "") if isinstance(value, dict) else None


def cmd_connect(args):
    opts, rest = options(
        args, ["root", "endpoint", "since", "log", "name"], ["root", "endpoint", "since", "log", "name"]
    )
    workspace, pane = rest
    since = int(opts["since"])
    ms = lambda: round((time.time_ns() - since) / 1e6, 1)
    if not inside(opts["root"], opts["endpoint"]):
        die(f"refusing endpoint {opts['endpoint']}: not under the bench's folder {opts['root']}")
    deadline = time.monotonic() + 120
    while not os.path.exists(opts["endpoint"]) or os.path.getsize(opts["endpoint"]) == 0:
        if time.monotonic() > deadline:
            die("no endpoint after 120 s")
        time.sleep(0.002)
    result = {"measure": opts["name"], "endpoint_ms": ms()}
    control = Control(opts["root"], opts["endpoint"])
    while True:
        text = shown(control, workspace, pane)
        if text is not None and text.strip():
            result["prompt_ms"] = ms()
            break
        if time.monotonic() > deadline:
            die(f"no text in {pane} after 120 s")
        time.sleep(0.003)
    # The connection's steps, as the app logged them (connect.rs `Laps`).
    with open(opts["log"], encoding="utf-8", errors="replace") as handle:
        for line in handle:
            if not line.startswith("nodes timing: "):
                continue
            fields = dict(f.split("=", 1) for f in line[len("nodes timing: "):].split() if "=" in f)
            step, at = fields.get("step"), fields.get("at")
            if not step or not at:
                continue
            at_ms = round(int(at) - since / 1e6, 1)
            if step == "begin":
                result.setdefault("begin_at_ms", at_ms)
            elif step == "stream":
                result.setdefault("stream_at_ms", at_ms)
            else:
                result.setdefault(f"{step}_ms", int(fields.get("ms", 0)))
                if step == "handshake":
                    result.setdefault("connected_at_ms", at_ms)
    print(json.dumps(result))


def last_line(control, workspace, pane):
    value = control.call("pane_read", {"workspace": workspace, "pane": pane, "lines": 3})
    lines = [line for line in value.get("text", "").split("\n") if line.strip()]
    return lines[-1] if lines else ""


def desktop_until(control, workspace, pane, needle, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = control.call("pane_read", {"workspace": workspace, "pane": pane, "lines": 5})
        if needle in value.get("text", ""):
            return
    die(f"{needle!r} did not show in {pane} within {timeout} s")


def desktop_send(control, workspace, pane, text, submit):
    body = {"workspace": workspace, "pane": pane, "text": text, "submit": submit}
    control.call("pane_send", body)


def cmd_desktop_prepare(args):
    opts, rest = options(args, ["root", "endpoint"], ["root", "endpoint"])
    workspace, pane = rest
    control = Control(opts["root"], opts["endpoint"])
    send = lambda text: desktop_send(control, workspace, pane, text, True)
    send("exec bash --norc --noprofile -i")
    time.sleep(0.5)
    send(f"PS1='{PROMPT}'; bind 'set enable-bracketed-paste off'; clear; echo re$((1+1))ady")
    desktop_until(control, workspace, pane, "re2ady")
    print(json.dumps({"prepared": pane}))


def cmd_desktop_echo(args):
    opts, rest = options(args, ["root", "endpoint", "samples"], ["root", "endpoint"])
    workspace, pane = rest
    samples = int(opts.get("samples", 200))
    control = Control(opts["root"], opts["endpoint"])
    typed = [""]

    def send(key):
        if key == "\x03":
            control.call("pane_send", {"workspace": workspace, "pane": pane, "keys": ["ctrl+c"]})
        else:
            desktop_send(control, workspace, pane, key, False)

    def wait_for(key):
        if key is None:
            typed[0] = ""
            while last_line(control, workspace, pane).rstrip() != PROMPT.rstrip():
                pass
            return
        typed[0] += key
        while not last_line(control, workspace, pane).endswith(PROMPT + typed[0]):
            pass

    send("\x03")
    wait_for(None)
    times = echo_samples(samples, send, wait_for)
    print(json.dumps({"measure": "desktop-echo", **summary(times)}))


def cmd_desktop_flood(args):
    opts, rest = options(args, ["root", "endpoint", "lines"], ["root", "endpoint"])
    workspace, pane = rest
    lines = int(opts.get("lines", 2_000_000))
    control = Control(opts["root"], opts["endpoint"])
    tag = f"{time.monotonic_ns() % 100000}"
    desktop_send(control, workspace, pane, f"seq 1 {lines}; echo fl{tag}$((1+1))od", True)
    start = time.perf_counter()
    desktop_until(control, workspace, pane, f"fl{tag}2od", timeout=600)
    seconds = time.perf_counter() - start
    print(json.dumps({"measure": "desktop-flood", "lines": lines, "seconds": round(seconds, 3)}))


# --- The report ---------------------------------------------------------------


def cmd_report(args):
    """The median of each number over the runs of each measure."""
    runs = {}
    with open(args[0], encoding="utf-8") as handle:
        for line in handle:
            line = line.strip()
            if not line.startswith("{"):
                continue
            value = json.loads(line)
            if "measure" in value:
                runs.setdefault(value["measure"], []).append(value)
    report = {}
    for measure, values in runs.items():
        keys = [k for k, v in values[0].items() if isinstance(v, (int, float))]
        report[measure] = {"runs": len(values)}
        for key in keys:
            numbers = [v[key] for v in values if isinstance(v.get(key), (int, float))]
            report[measure][key] = round(statistics.median(numbers), 3)
    width = max((len(m) for m in report), default=0)
    for measure, value in report.items():
        fields = ", ".join(f"{k} {v}" for k, v in value.items() if k != "runs")
        print(f"{measure:<{width}}  ({value['runs']} runs)  {fields}")
    with open(args[1], "w", encoding="utf-8") as handle:
        json.dump(report, handle, indent=2)


COMMANDS = {
    "node-echo": cmd_node_echo,
    "node-flood": cmd_node_flood,
    "node-spawn": cmd_node_spawn,
    "node-kill": cmd_node_kill,
    "local-echo": cmd_local_echo,
    "desktop-prepare": cmd_desktop_prepare,
    "desktop-echo": cmd_desktop_echo,
    "desktop-flood": cmd_desktop_flood,
    "connect": cmd_connect,
    "report": cmd_report,
}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        print(__doc__, file=sys.stderr)
        sys.exit(2)
    COMMANDS[sys.argv[1]](sys.argv[2:])
