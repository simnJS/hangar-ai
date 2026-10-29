import type { ChatItem, ChatQuestion } from "../remote/types";

/**
 * Answering Claude Code's AskUserQuestion from the phone (remote_answer in
 * lib/remoteControl): the window types into the question's dialog what the
 * person would type at the PC, so the session gets the answer exactly as if
 * it had been given there.
 *
 * The keys are those of Claude Code's own dialog (2.1.x). It shows one tab
 * per question, each opening with the cursor on its first option. Enter picks
 * a single-select option and moves on to the next tab; on a multi-select
 * question it ticks the option under the cursor, and the row below the
 * options, "Next" ("Submit" on the last), moves on. The row just after the
 * options, "Type something", takes the person's own words as they are pasted,
 * and ticks itself. A review then waits for Enter on "Submit answers", except
 * after a lone single-select question, which its Enter submits.
 *
 * Every step is checked on the screen before the next: nothing is typed
 * unless the dialog shows the question as Claude Code first drew it, and
 * nothing is submitted unless the screen shows what the person chose. A
 * dialog someone is answering at the PC, a key lost on the way, a Claude Code
 * that draws its dialog otherwise: each stops the answer, never submitting
 * what the person did not choose.
 */

/** One write into the pane: `text` pasted, never followed by Enter, then `keys`. */
export interface AnswerStep {
  text?: string;
  keys: string[];
}

/** A question's answer, checked against its options: the indexes picked, ascending. */
export interface Choice {
  picked: number[];
  other?: string;
}

/** What the pane reads and types, through pane_read and pane_send. */
export interface AnswerIo {
  /** The last lines the pane's terminal shows. */
  read: () => Promise<string>;
  type: (step: AnswerStep) => Promise<void>;
  sleep: (ms: number) => Promise<void>;
}

/** How long a step gets to show on the screen, read again every SETTLE_MS. */
const SETTLE_MS = 150;
const SETTLE_TRIES = 20;
/** The question is on the screen already when the transcript says it waits. */
const FIRST_TRIES = 3;

export const NOT_ON_SCREEN =
  "The question is not on the PC's screen as Claude Code first drew it: someone may be answering it there. Nothing was typed.";
export const NOT_REVIEWED =
  "The PC's screen does not show these answers ready to submit, so nothing was submitted: check them in the Terminal view.";
export const lostAt = (index: number) =>
  `Question ${index + 1} did not come up on the PC as expected, so the rest was not typed: finish in the Terminal view.`;

/**
 * The questions `toolUseId` asks, when the pane waits on it; otherwise the
 * reason it does not, thrown for the phone to show: its screen is stale.
 */
export function pendingQuestions(
  pending: { toolUseId: string; questions: ChatQuestion[] } | null,
  items: ChatItem[],
  toolUseId: string,
): ChatQuestion[] {
  if (pending?.toolUseId === toolUseId) return pending.questions;
  const asked = items.find((item) => item.kind === "question" && item.toolUseId === toolUseId);
  if (asked?.kind === "question" && asked.answer) throw new Error("This question was already answered.");
  if (asked) throw new Error("Claude Code no longer waits on this question: it was dismissed.");
  if (pending) throw new Error("Claude Code now waits on another question.");
  throw new Error("Claude Code is not waiting on this question.");
}

/** Line breaks and control characters out: in the dialog's field a newline would answer. */
const cleanWords = (text: string) =>
  text
    .replace(/[\r\n\t]+/g, " ")
    .replace(/[\u0000-\u001f\u007f-\u009f]/g, "")
    .trim();

/** The phone's answers held against the questions; what does not fit is refused. */
export function choicesOf(questions: ChatQuestion[], answers: unknown): Choice[] {
  if (!Array.isArray(answers) || answers.length !== questions.length) {
    throw new Error(
      questions.length === 1 ? "Give one answer: this is one question." : `Give one answer per question: ${questions.length} of them.`,
    );
  }
  return questions.map((question, index) => {
    const name = questions.length === 1 ? `"${question.header}"` : `Question ${index + 1} ("${question.header}")`;
    const answer = (answers[index] ?? {}) as { selected?: unknown; other?: unknown };
    const selected = answer.selected ?? [];
    if (!Array.isArray(selected) || selected.some((label) => typeof label !== "string")) {
      throw new Error(`${name}: "selected" is a list of option labels.`);
    }
    if (answer.other !== undefined && typeof answer.other !== "string") {
      throw new Error(`${name}: "other" is the person's own words, as text.`);
    }
    const picked = [...new Set(selected as string[])]
      .map((label) => {
        const at = question.options.findIndex((option) => option.label === label);
        if (at < 0) throw new Error(`"${label}" is not one of the options of ${name}.`);
        return at;
      })
      .sort((a, b) => a - b);
    const other = cleanWords(answer.other ?? "") || undefined;
    const given = picked.length + (other ? 1 : 0);
    if (given === 0) throw new Error(`${name} has no answer.`);
    if (!question.multiSelect && given > 1) {
      throw new Error(`${name} takes one answer: one option, or your own words.`);
    }
    return other ? { picked, other } : { picked };
  });
}

const down = (count: number) => Array.from({ length: count }, () => "down");

/**
 * What answering one question types, from its tab as first drawn: the cursor
 * on the first option. The last key is the Enter that moves on, or submits.
 */
export function stepsFor(question: ChatQuestion, choice: Choice): AnswerStep[] {
  const typeRow = question.options.length;
  if (!question.multiSelect) {
    if (choice.other !== undefined) return [{ keys: down(typeRow) }, { text: choice.other, keys: ["enter"] }];
    return [{ keys: [...down(choice.picked[0]), "enter"] }];
  }
  const keys: string[] = [];
  let at = 0;
  for (const index of choice.picked) {
    keys.push(...down(index - at), "enter");
    at = index;
  }
  // "Next" is the row after "Type something".
  if (choice.other === undefined) return [{ keys: [...keys, ...down(typeRow + 1 - at), "enter"] }];
  return [
    { keys: [...keys, ...down(typeRow - at)] },
    { text: choice.other, keys: ["down", "enter"] },
  ];
}

/* ── Reading the dialog off the screen ────────────────────────────── */

const squash = (text: string) => text.replace(/\s+/g, "");

/** A numbered row, or the unnumbered one that moves on ("Next", "Submit"). */
interface Row {
  n: number | "next";
  cursor: boolean;
  text: string;
  /** Its line in the dialog. */
  at: number;
}

const NUMBERED = /^\s*([❯>])?\s*(\d+)\.\s+(.*?)\s*$/;
const MOVE_ON = /^\s*([❯>])?\s+(Next|Submit)\s*$/;
const BOX = /^\[(.)\]\s*/;

function rowsOf(lines: string[]): Row[] {
  const rows: Row[] = [];
  lines.forEach((line, at) => {
    const numbered = NUMBERED.exec(line);
    if (numbered) rows.push({ n: Number(numbered[2]), cursor: Boolean(numbered[1]), text: numbered[3], at });
    const moveOn = MOVE_ON.exec(line);
    if (moveOn) rows.push({ n: "next", cursor: Boolean(moveOn[1]), text: moveOn[2], at });
  });
  return rows;
}

/**
 * The dialog: the lines below its tab bar, the last line of the screen that
 * shows one of the questions' headers as a tab (☐ unanswered, ☒ answered).
 * Claude Code draws it at the bottom, in place of the prompt.
 */
export function dialogOf(screen: string, questions: ChatQuestion[]): string[] | null {
  const lines = screen.split("\n");
  for (let at = lines.length - 1; at >= 0; at--) {
    const line = lines[at];
    if (questions.some((q) => line.includes(`☐ ${q.header}`) || line.includes(`☒ ${q.header}`))) {
      return lines.slice(at + 1);
    }
  }
  return null;
}

/**
 * Whether the dialog shows `question` as first drawn: its options in their
 * order, nothing ticked, nothing typed, the cursor on the first.
 */
export function untouched(lines: string[], question: ChatQuestion): boolean {
  const rows = rowsOf(lines);
  const first = rows.find((row) => row.n === 1);
  if (!first?.cursor || rows.filter((row) => row.cursor).length !== 1) return false;
  if (!squash(lines.slice(0, first.at).join("")).includes(squash(question.question))) return false;
  const numbered = (n: number) => rows.find((row) => row.n === n);
  const optionsShown = question.options.every((option, index) => {
    const row = numbered(index + 1);
    if (!row) return false;
    const box = BOX.exec(row.text);
    if (question.multiSelect ? box?.[1] !== " " : box) return false;
    return squash(box ? row.text.slice(box[0].length) : row.text) === squash(option.label);
  });
  const typeRow = numbered(question.options.length + 1);
  return optionsShown && typeRow !== undefined && /^(\[ \]\s*)?Type something/.test(typeRow.text);
}

/**
 * Whether a lone single-select question has the cursor on the person's
 * answer: its Enter submits at once, with no review to check first.
 */
export function chosen(lines: string[], question: ChatQuestion, choice: Choice): boolean {
  const cursor = rowsOf(lines).filter((row) => row.cursor);
  if (cursor.length !== 1) return false;
  const [row] = cursor;
  if (choice.other === undefined) {
    const index = choice.picked[0];
    return row.n === index + 1 && squash(row.text) === squash(question.options[index].label);
  }
  // Words longer than the row run on below it.
  const runOn: string[] = [];
  for (const line of lines.slice(row.at + 1)) {
    if (!line.trim() || line.includes("─") || NUMBERED.test(line) || MOVE_ON.test(line)) break;
    runOn.push(line);
  }
  return row.n === question.options.length + 1 && squash([row.text, ...runOn].join("")) === squash(choice.other);
}

/**
 * Whether the review shows, for each question, the answer asked for and
 * nothing else, with the cursor on "Submit answers". Claude Code joins a
 * question's answers with ", " and may quote the person's words.
 */
export function reviewed(lines: string[], questions: ChatQuestion[], choices: Choice[]): boolean {
  const text = lines.join("\n");
  if (!squash(text).includes("Reviewyouranswers")) return false;
  const submit = rowsOf(lines).find((row) => row.n === 1);
  if (!submit?.cursor || !submit.text.startsWith("Submit answers")) return false;
  const end = text.indexOf("Ready to submit");
  const parts = (end < 0 ? text : text.slice(0, end)).split("●").slice(1).map(squash);
  return questions.every((question, index) => {
    const asked = squash(question.question);
    const part = parts.find((entry) => entry.startsWith(asked + "→"));
    if (!part) return false;
    let left = part.slice(asked.length + 1);
    const choice = choices[index];
    const pieces = [...choice.picked.map((at) => question.options[at].label), ...(choice.other ? [choice.other] : [])]
      .map(squash)
      .sort((a, b) => b.length - a.length);
    for (const piece of pieces) {
      const at = left.indexOf(piece);
      if (at < 0) return false;
      left = left.slice(0, at) + left.slice(at + piece.length);
    }
    return /^[,"]*$/.test(left);
  });
}

/* ── Typing it ────────────────────────────────────────────────────── */

/** Reads the screen until the dialog passes `check`, a few times. */
async function shows(
  io: AnswerIo,
  questions: ChatQuestion[],
  check: (lines: string[]) => boolean,
  tries = SETTLE_TRIES,
): Promise<boolean> {
  for (let attempt = 0; attempt < tries; attempt++) {
    if (attempt > 0) await io.sleep(SETTLE_MS);
    const lines = dialogOf(await io.read(), questions);
    if (lines && check(lines)) return true;
  }
  return false;
}

/**
 * Types the answers into the dialog of `questions`, checking the screen
 * before each question and before the Enter that submits. Throws, having
 * submitted nothing, when the screen is not what that step expects.
 */
export async function typeAnswers(io: AnswerIo, questions: ChatQuestion[], choices: Choice[]): Promise<void> {
  const lone = questions.length === 1 && !questions[0].multiSelect;
  for (const [index, question] of questions.entries()) {
    const ready = await shows(io, questions, (lines) => untouched(lines, question), index === 0 ? FIRST_TRIES : SETTLE_TRIES);
    if (!ready) throw new Error(index === 0 ? NOT_ON_SCREEN : lostAt(index));
    const steps = stepsFor(question, choices[index]);
    if (lone) {
      // Held back: this Enter submits, so the screen is checked first.
      const last = steps[steps.length - 1];
      steps[steps.length - 1] = { ...last, keys: last.keys.slice(0, -1) };
    }
    for (const step of steps) if (step.text || step.keys.length > 0) await io.type(step);
  }
  const ready = lone
    ? await shows(io, questions, (lines) => chosen(lines, questions[0], choices[0]))
    : await shows(io, questions, (lines) => reviewed(lines, questions, choices));
  if (!ready) throw new Error(NOT_REVIEWED);
  await io.type({ keys: ["enter"] });
}
