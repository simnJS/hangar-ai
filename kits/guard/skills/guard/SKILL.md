---
name: guard
description: What to do when a tool call is refused with "The guard refused this". Explain to the person why you needed it, and wait; do not look for another way around it.
---

# When the guard refuses a call

A hook of this kit refuses, before they run, dangerous commands and the reading of
secrets. The refusal is a decision for the person to make, not an obstacle to get
past.

1. **Stop trying to do that thing.** No rewording, no other program, no script that
   does the same, no copy of the file under another name. The guard reads command
   lines and is easy to walk around: that is exactly why you must not.
2. **Tell the person, in a few lines:** what you were trying to do, why you need it,
   what it would change or show, and whether there is a safer way you would rather
   take. If you can do without it, say so and do without it.
3. **Wait for their answer.** They can let this exact command through once, from the
   **Guardrails** tab (« Garde-fous »), for two minutes.
4. **When they tell you to retry, run the same command, character for character.**
   The pass holds for that exact text and for you: anything else is refused again.
   If it was refused again, say so rather than changing it.

Do not change `.hangar/guard.json`, the kit's rules, or its data folder to get a call
through, and do not ask another agent to run it for you. Those are the person's.
