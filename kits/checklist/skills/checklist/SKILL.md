---
name: checklist
description: When your work ends in something only a person can check (how a screen looks, a flow in the real app, a device, a sound), hand them a checklist in Hangar's Recette tab and wait for the result, instead of ending with "please test this" in chat.
---

# Handing the person a checklist

Do not finish with "please test this" in the chat: nobody re-reads it, and the result never
comes back to you. List what only a human can check, open it as a tab, and stop.

## Do this

1. Write the list as a JSON file inside the workspace, in a folder that is not hidden (not
   `.something/`), for example `recette/login.json`:

   ```json
   {
     "title": "Login and logout",
     "steps": [
       {
         "text": "Open /login, enter a valid email with a wrong password, press Sign in.",
         "expect": "\"Invalid credentials\" appears under the field; the page does not reload."
       },
       {
         "text": "Sign in with the test account, then press Back in the browser.",
         "expect": "You stay on the dashboard; the account name shows top right."
       }
     ]
   }
   ```

   `text` is required; `expect` is what the person should see. Up to 200 steps.
2. Call the `hangar` MCP tool `kit_tab_open`:

   ```json
   { "kit": "checklist", "tab": "checklist", "params": { "path": "recette/login.json" } }
   ```

   The path is relative to the workspace folder, with no spaces or colons. One tab per path.
3. **Stop.** End your turn, or do unrelated work. Do not poll and do not ask in chat. The
   person marks each step OK, Problem (with a note) or Skip, then presses "Send to agent": a
   message from `kit:checklist` reaches you with what failed, their notes, and what was skipped.

## Writing the steps

- **One action, one thing to see.** "Open the settings, turn on Dark mode" and "the whole
  window turns dark, including the sidebar" — not "check that settings work".
- **Name the real thing**: the page, the button's label, the value to type, the account to use.
- **Say what is expected** in `expect`, concretely enough that a failure is unmistakable.
- **Only what a person must check.** What a test, a build or a screenshot of yours can check,
  you check yourself first. Put the rest here, in the order a person would do it.
- No vague steps ("check it works", "try a few things").

## When the answer comes

1. Read the failed steps and their notes. A note is what the person saw: take it as the symptom.
2. Fix what failed. Do not touch the passed steps.
3. Ask for the **same path** again with `kit_tab_open` and the same params. The tab keeps the
   answers, puts the failed steps back as "To redo", and you get a new summary when sent.
   If you changed the wording of a step, it counts as a new step; edit the file only to add steps.
4. Repeat until nothing failed. Skipped steps are the person's call: mention them in your report.

If `kit_tab_open` is refused (the kit is not enabled on this workspace, or waits for the
person to accept something), say so in one line and put the list in your answer instead.
