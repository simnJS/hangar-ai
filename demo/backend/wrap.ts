/**
 * Line wrapping for the agents' transcripts, the way Claude Code lays out its
 * own: at a word, with the continuation under the text rather than under the
 * bullet. Left to the terminal, a long tool call would break in the middle of
 * a word and run back under the ⏺.
 *
 * Colours are escape sequences with no width, and never contain a space, so a
 * line can be cut at its spaces without ever cutting one.
 */

// eslint-disable-next-line no-control-regex
const ESCAPES = /\x1b\[[0-9;?]*[A-Za-z]/g;
// eslint-disable-next-line no-control-regex
const ESCAPE_TOKEN = /(\x1b\[[0-9;?]*[A-Za-z])/;

const plain = (text: string) => text.replace(ESCAPES, "");

export const visibleWidth = (text: string) => [...plain(text)].length;

/** What a line starts with, that its continuation rows sit under. */
const MARKER = /^( *)(⏺ |⎿ {2}|❯ |- |✓ |✗ |\d+\. )?/;

/**
 * A word too long for a row, cut by visible characters, colours kept: the
 * first piece `first` wide, the others `width`.
 */
function cutWord(word: string, width: number, first = width): string[] {
  const parts: string[] = [];
  let part = "";
  let used = 0;
  let room = Math.max(first, 1);
  for (const token of word.split(ESCAPE_TOKEN)) {
    if (ESCAPE_TOKEN.test(token)) {
      part += token;
      continue;
    }
    for (const char of token) {
      if (used === room) {
        parts.push(part);
        part = "";
        used = 0;
        room = width;
      }
      part += char;
      used += 1;
    }
  }
  parts.push(part);
  return parts;
}

/**
 * One line, without a line break in it, as the rows it takes at `cols`.
 * `before` is what the row it starts on already shows (a prompt, say): the
 * first row shares it, and the continuation is indented under it.
 */
export function wrapLine(line: string, cols: number, before = ""): string[] {
  const width = Math.max(cols, 20);
  const start = visibleWidth(before);
  if (!line || start + visibleWidth(line) <= width) return [line];

  const hang = Math.min(MARKER.exec(plain(before + line))?.[0].length ?? 0, Math.floor(width / 2));
  const indent = " ".repeat(hang);
  const rows: string[] = [];
  let row = "";
  let used = start;
  /** No word on the row yet: the next one goes without a space before it. */
  let fresh = true;
  /** Something other than indentation on the row: a break here leaves a row worth having. */
  let shown = /\S/.test(plain(before));

  for (const word of line.split(" ")) {
    const size = visibleWidth(word);
    const gap = fresh ? 0 : 1;
    if (used + gap + size <= width) {
      row += (gap ? " " : "") + word;
      used += gap + size;
      fresh = false;
      shown ||= size > 0;
      continue;
    }
    // Nothing but indentation yet: the word starts on this row, cut to fit,
    // rather than leaving a row of spaces behind.
    const pieces = shown
      ? size + hang <= width
        ? [word]
        : cutWord(word, width - hang)
      : cutWord(word, width - hang, width - used - gap);
    if (shown) {
      rows.push(row);
      // The space the line broke at is the line break.
      if (size === 0) {
        row = indent;
        used = hang;
        fresh = true;
        shown = false;
        continue;
      }
      row = indent;
    } else {
      row += gap ? " " : "";
    }
    for (const piece of pieces.slice(0, -1)) {
      rows.push(row + piece);
      row = indent;
    }
    const last = pieces[pieces.length - 1];
    row += last;
    used = visibleWidth(row);
    fresh = false;
    shown = true;
  }
  rows.push(row);
  return rows;
}

/**
 * A chunk of output, its lines wrapped at `cols`. `tail` is the visible text
 * already on the row the chunk starts on.
 */
export function softWrap(text: string, cols: number, tail = ""): string {
  return text
    .split("\r\n")
    .map((line, index) => wrapLine(line, cols, index === 0 ? tail : "").join("\r\n"))
    .join("\r\n");
}

/** What the row the cursor ends on shows, once `text` is printed after `tail`. */
export function tailAfter(tail: string, text: string): string {
  const at = text.lastIndexOf("\r\n");
  return at < 0 ? tail + plain(text) : plain(text.slice(at + 2));
}
