# Security policy

## Supported versions

Only the latest release receives security fixes. The app updates itself, so the fix
reaches you once you accept the update.

| Version | Supported |
|---|---|
| Latest release ([releases/latest](https://github.com/simnJS/hangar-ai/releases/latest)) | Yes |
| Anything older | No: please update first |

## Reporting a vulnerability

**Please do not open a public issue for a security problem.**

Report it privately through GitHub Security Advisories. Open the repository's
[**Security** tab](https://github.com/simnJS/hangar-ai/security), click **Report a
vulnerability**, and fill in the form. Only the maintainer can see the report.

Please include:

- the Hangar version and your operating system;
- what an attacker can do, and what they need first (local access, a malicious repository,
  a crafted MCP request…);
- steps to reproduce, or a proof of concept;
- any fix or mitigation you have in mind.

Hangar is maintained by one person, so please allow some time for an answer. Once the issue
is confirmed, a fixed release is published, then the advisory is made public with credit
to you, unless you prefer to stay anonymous.

## Scope

These are in scope:

- the desktop app, including its local HTTP API (`127.0.0.1`, protected by a per-launch
  token) and the `hangar-ai --mcp` server;
- the `hangar-bridge` Claude Code mod in `src-tauri/claude-mod/`;
- the installers, the updater and the release workflow in this repository.

These are out of scope, and should be reported to their own projects:

- the agent CLIs Hangar launches (Claude Code, Codex, Gemini CLI, OpenCode);
- what an agent does with the permissions you gave it. Hangar runs agents with your user
  account and does not sandbox them.
