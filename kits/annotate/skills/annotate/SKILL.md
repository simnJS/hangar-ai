---
name: annotate
description: When you need the person's eye on something visual (a layout that looks wrong, a misplaced button, a rendering bug, a screen to compare), take a capture and open it in Hangar's Annoter tab for them to circle the problem and write a word beside it. Use it instead of asking them to describe a visual problem in words.
---

# Getting the person's eye on something visual

Describing "the second button is too close to the edge on the settings page" takes the person
a paragraph and still leaves you guessing. A capture they can circle takes ten seconds.

## Do this

1. Take the capture and save it **inside the workspace folder**, as a PNG, JPEG, WebP or GIF:
   a screenshot from the browser or the app you are driving, for example `.hangar/capture.png`.
   Look at it yourself first: do not ask the person about an image you have not seen.
2. Call the `hangar` MCP tool `kit_tab_open`:

   ```json
   { "kit": "annotate", "tab": "annotate", "params": { "path": ".hangar/capture.png" } }
   ```

   `path` is relative to the workspace folder. Every param is a string.
3. Wait. End your turn, or do other work: a message from `kit:annotate` tells you when the
   person sends. Say in your own chat message what you want them to look at, because the tab
   only shows the image.
4. The message gives the path of the annotated PNG (`.hangar/annotations/<date>-<time>.png`),
   the person's comment and their numbered notes with the position of each. **Read the PNG**:
   the marks (arrows, rectangles, strokes, numbered badges) say where, the notes say what.
   `.hangar/annotations/<date>-<time>.json` holds the same, with a line per mark.
5. Fix what they marked, then take a new capture. If it is still wrong, open the tab again
   with the new path: a new image is a new tab.

## Never

- Never open the tab on an image outside the workspace folder, or on an SVG or a PDF: it
  opens bitmap images only.
- Never treat a missing answer as "no problem". The person may not have looked yet.

If `kit_tab_open` is refused (the kit is not enabled on this workspace, or waits for the
person to accept something), say so in one line and describe what you wanted them to look at.
