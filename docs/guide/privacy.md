# Privacy

[← Hangar](../../README.md) · [Guide](README.md)

Hangar has no account, no telemetry and no backend. Your workspaces, the board, the memory
and the statistics stay on your machine:

- each workspace's board is in `.hangar/board.json` in the project. Hangar keeps it out of
  `git status` through `.git/info/exclude`, unless you commit it yourself;
- the app state and the memory are in the app's data folder;
- the statistics are computed from the transcripts the agents already keep locally, and
  are never sent anywhere;
- the MCP API listens on `127.0.0.1` only, behind a token that changes at each launch;
- [Hangar Mobile](mobile.md), off until you turn it on, listens on `127.0.0.1` too, and
  only your own devices reach it, through Tailscale.

Hangar itself only goes online in these cases:

- **Updates.** It checks the GitHub release feed for a new version.
- **Dictation.** It downloads the speech model from Hugging Face once, when you choose it.
  It sends audio to Groq only if you choose Groq as the engine, and the transcript only if
  you turn on the optional cleanup pass, which also runs on Groq.
- **Discord.** Rich Presence talks to your local Discord client, and only if you turn it
  on.
- **Hangar Mobile notifications.** Once you turn them on on a phone, the PC sends them
  through that phone's push service (Apple's, Google's or Mozilla's), encrypted for that
  phone alone, so the push service cannot read them.

The agents you run talk to their own providers under their own settings. Hangar does not
change that.
