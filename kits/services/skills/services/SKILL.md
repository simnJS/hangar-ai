---
name: services
description: When a port is already taken, a dev server or a container is left running, or you need to know what runs on this computer and whose it is, open Hangar's Services tab for the person instead of guessing with netstat or lsof. Use it before starting a server outside your own port block.
---

# What is running, and whose is it

Workers each get 20 ports (`HANGAR_PORT_BASE` to `HANGAR_PORT_BASE + 19`), so leftovers pile up.
The Services tab lists the listening ports with their program, pid and start time, names the
worker whose port block a port is in, and lists Docker containers with their logs.

## Do this

1. Call the `hangar` MCP tool `kit_tab_open` with `{ "kit": "services", "tab": "services" }`. It takes
   no param, and only reads: it stops nothing.
2. Say in one line what you are looking for (a port, a container), so the person knows where to look.
3. Stop only what you started, in your own port block. For anything else, leave it: the person
   can use « Ask an agent to stop this » in the tab, which sends the brief to the agent they pick.

## Never

- Never kill a process or stop a container you did not start, even if it holds the port you want.
  Use a free port of your own block instead.
- Never start a server on a default port (3000, 5173, 8080): use your block.

If `kit_tab_open` is refused (the kit is not enabled on this workspace, or waits for the person to
accept something), say so in one line and carry on with your own ports.
