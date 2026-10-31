# Fixtures

What each tool prints, scrubbed: pids, names, addresses and times are invented, and no path or name of a person
is in them. Which are recorded and which are not:

| File                    | What it is                                                                                                  |
| ----------------------- | ----------------------------------------------------------------------------------------------------------- |
| `windows-listen.json`   | The shape recorded on Windows 11 (`Get-NetTCPConnection` and `Get-CimInstance` through `ConvertTo-Json`), a handful of entries with invented values and documentation addresses. |
| `windows-single.json`   | The same, with one item each: PowerShell writes a lone item as an object, not a list. |
| `linux-ss.txt`          | The first five lines are `ss -H -ltnp` recorded on Ubuntu (no process: another account's sockets). The others are written in the format `ss -p` documents, with a process. |
| `linux-ps.txt`          | `ps -o pid=,lstart=` under `LC_ALL=C`, the format recorded, the values invented. |
| `linux-environ.bin`     | A NUL-separated environment as `/proc/<pid>/environ` holds it, written; its `SECRET_TOKEN` is there to prove it is never kept. |
| `macos-lsof.txt`        | `lsof -F pcn`, written from lsof's documented field output (no macOS here to record it). |
| `macos-ps-eww.txt`      | `ps eww`, written from its documented output, same reason. |
| `docker-down.txt`       | Recorded: what `docker ps` prints on Windows when Docker Desktop is not running. |
| `docker-down-unix.txt`  | Written from the documented message of a stopped engine on Linux and macOS. |
| `docker-ps.jsonl`       | `docker ps -a --format json`, written from the documented fields (the daemon was off here, and the tests never start it). |
| `docker-logs.txt`       | A container's log, written, with an error and its stack in the middle. |
