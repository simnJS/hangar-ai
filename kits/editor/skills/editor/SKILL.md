---
name: editor
description: When you need a secret or a value you cannot or must not set yourself (an API key, a token, a password, a machine setting), or a config file the person should check, open it in Hangar's Éditeur tab for them to fill in. Use it instead of asking for the value in chat or writing a placeholder.
---

# Getting a value you must not see

Do not ask for a secret in chat: the conversation keeps it. Do not write a placeholder
(`API_KEY=changeme`) and carry on: the next run fails later, further from the cause.

## Do this

1. Call the `hangar` MCP tool `kit_tab_open`:

   ```json
   {
     "kit": "editor",
     "tab": "editor",
     "params": {
       "path": ".env",
       "keys": "STRIPE_KEY,DB_URL",
       "why": "The payment tests need a test-mode key, from the Stripe dashboard."
     }
   }
   ```

   Every param is a string. `keys` is a comma-separated list, asked for all at once: other
   params open another tab. For another text file, give `path`, and `line` to point at a
   line, and say in `why` what the person should check.
2. Wait. End your turn, or do other work: a message from `kit:editor` tells you when the
   person saved, and which keys were set. It holds no value, and it is the only answer.
3. Carry on through the program that loads the file.

## Never

- Never read the values back: no `cat .env`, no `grep`, no `printenv`, no printing them
  from your code or your tests, not even to check them. If it fails, say which key seems
  wrong and open the tab again.
- Never write the values anywhere yourself, and never pass them as arguments.

If `kit_tab_open` is refused (the kit is not enabled on this workspace, or waits for the
person to accept something), say so in one line and tell the person which file and keys you
need. Do not ask for the value itself.
