# Nodes test rig

A Docker container with sshd, reachable on `127.0.0.1`, to test hangar-node end
to end without touching `~/.ssh` or a real server. Everything it needs lives here:
a dedicated key pair and known_hosts in `.keys/` (gitignored).

| Script | What it does |
| --- | --- |
| `up.sh [--arm64] [--with-claude]` | generates the key once, builds the image, runs container `hangar-node-test`, waits for ssh, prints the target and options |
| `down.sh` | removes the container (keeps the keys) |
| `ssh.sh <cmd>` | runs a command on the node with the right options |
| `check.sh [--arm64]` | smoke test: up, `uname -m`, `whoami`, scp round trip, password/root refused, down |

- Image: pinned Debian (dated tag + digest), openssh-server, git, bash, curl,
  ca-certificates, procps; user `hangar`; key auth only, no root login, port 22.
- Port: `HANGAR_NODE_TEST_PORT`, else the first free one from 2222. Container
  name: `HANGAR_NODE_TEST_CONTAINER` (default `hangar-node-test`).
- `--with-claude` adds Node.js and `@anthropic-ai/claude-code` (off by default;
  no Claude login happens in tests).
- `--arm64` runs a `linux/arm64` container through qemu-user-binfmt. If this
  machine cannot run arm64 containers it says so and exits 0 without starting anything.
- One container at a time: `up.sh` replaces a running one.

## Pointing code at it

`up.sh` writes `scripts/node-test/ssh_config` (gitignored, absolute paths, current port):

```
ssh -F scripts/node-test/ssh_config hangar-node-test
```

or the plain form `up.sh` prints:

```
ssh -i scripts/node-test/.keys/id_ed25519 -p <port> -o StrictHostKeyChecking=no \
    -o UserKnownHostsFile=scripts/node-test/.keys/known_hosts hangar@127.0.0.1
```

The port is also in `scripts/node-test/.keys/port` while the rig is up.
`down.sh` removes `ssh_config` and `port`.
