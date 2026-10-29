import { describe, expect, it } from "vitest";
import type { ControlDeps } from "./control";
import {
  choicesOf,
  dialogOf,
  lostAt,
  NOT_ON_SCREEN,
  NOT_REVIEWED,
  pendingQuestions,
  stepsFor,
  typeAnswers,
  untouched,
  type AnswerIo,
  type AnswerStep,
} from "./remoteAnswer";
import { noteBridgeEvent, runRemote, type ChatPage } from "./remoteControl";
import type { ChatAnswer, ChatItem, ChatQuestion } from "../remote/types";
import { DEFAULT_SETTINGS, type AppState } from "../types";

const question = (header: string, text: string, labels: string[], multiSelect = false): ChatQuestion => ({
  question: text,
  header,
  multiSelect,
  options: labels.map((label) => ({ label, description: `The ${label.toLowerCase()}` })),
});

const COLOR = question("Color", "Which color do you prefer?", ["Red", "Green", "Blue"]);
const FRUITS = question("Fruits", "Which fruits do you like?", ["Apple", "Banana", "Cherry", "Date"], true);
const PET = question("Pet", "Which pet would you adopt?", ["Cat", "Dog"]);
const SIZE = question("Size", "Which size?", ["Small", "Large"]);
const TOOLS = question("Tools", "Which tools?", ["Hammer", "Saw", "Drill"], true);

const RULE = "─".repeat(80);

/**
 * Claude Code's AskUserQuestion dialog as a pane shows it, row for row as
 * 2.1.312 draws it (wrapped rows joined back, as pane_read gives them), and
 * how it takes keys and pasted text. The transcript above it mentions the
 * questions and has a todo on a header, as a real screen may.
 */
class Dialog {
  tab = 0;
  cursor = 0;
  reviewing = false;
  /** Question text → the answer string Claude Code records; "cancelled"; or null. */
  submitted: Record<string, string> | "cancelled" | null = null;
  /** Every key that reached it. */
  readonly keys: string[] = [];
  private readonly picked: (string | null)[];
  private readonly ticked: Set<number>[];
  private readonly typed: string[];
  private readonly typedTicked: boolean[];
  private readonly done: boolean[];

  /** `lose`: the indexes of keys that never reach it. */
  constructor(
    readonly questions: ChatQuestion[],
    private readonly lose: number[] = [],
  ) {
    this.picked = questions.map(() => null);
    this.ticked = questions.map(() => new Set());
    this.typed = questions.map(() => "");
    this.typedTicked = questions.map(() => false);
    this.done = questions.map(() => false);
  }

  private get lone() {
    return this.questions.length === 1 && !this.questions[0].multiSelect;
  }

  private answerOf(index: number): string {
    const q = this.questions[index];
    if (!q.multiSelect) return this.picked[index] ?? "";
    const words = this.typed[index];
    return [
      ...q.options.filter((_, at) => this.ticked[index].has(at)).map((option) => option.label),
      ...(this.typedTicked[index] && words ? [words.includes(", ") ? `"${words}"` : words] : []),
    ].join(", ");
  }

  private moveOn() {
    this.done[this.tab] = true;
    if (this.lone) return this.submit();
    this.tab++;
    this.cursor = 0;
    if (this.tab === this.questions.length) this.reviewing = true;
  }

  private submit() {
    this.submitted = Object.fromEntries(this.questions.map((q, at) => [q.question, this.answerOf(at)]));
  }

  key(name: string) {
    this.keys.push(name);
    if (this.lose.includes(this.keys.length - 1) || this.submitted) return;
    const q = this.questions[this.tab];
    const typeRow = q?.options.length ?? 0;
    const last = this.reviewing ? 1 : q.multiSelect ? typeRow + 2 : typeRow + 1;
    if (name === "down") this.cursor = Math.min(last, this.cursor + 1);
    else if (name === "up") this.cursor = Math.max(0, this.cursor - 1);
    else if (name === "escape") this.submitted = "cancelled";
    else if (name !== "enter") throw new Error(`The dialog takes no "${name}".`);
    else if (this.reviewing) {
      if (this.cursor === 0) this.submit();
      else this.submitted = "cancelled";
    } else if (this.cursor === last) throw new Error('"Chat about this" was chosen.');
    else if (!q.multiSelect) {
      if (this.cursor < typeRow) this.picked[this.tab] = q.options[this.cursor].label;
      else if (this.typed[this.tab]) this.picked[this.tab] = this.typed[this.tab];
      else return;
      this.moveOn();
    } else if (this.cursor < typeRow) {
      const ticked = this.ticked[this.tab];
      if (ticked.has(this.cursor)) ticked.delete(this.cursor);
      else ticked.add(this.cursor);
    } else if (this.cursor === typeRow) this.typedTicked[this.tab] = !this.typedTicked[this.tab];
    else this.moveOn();
  }

  /** Text lands in "Type something" alone, and ticks it. */
  paste(text: string) {
    if (this.submitted || this.reviewing) return;
    if (this.cursor !== this.questions[this.tab].options.length) return;
    this.typed[this.tab] += text;
    this.typedTicked[this.tab] = true;
  }

  screen(): string {
    const lines = [
      `❯ Ask me: ${this.questions.map((q) => q.question).join(" ")}`,
      "",
      `  ☐ ${this.questions[0].header} the todo list`,
      "",
    ];
    if (this.submitted) {
      lines.push("● User answered Claude's questions:", "", RULE, "❯ ", RULE);
      return lines.join("\n");
    }
    const tabs = this.questions.map((q, at) => `${this.done[at] ? "☒" : "☐"} ${q.header}`).join("  ");
    lines.push(this.lone ? `${RULE} ${tabs} ` : `${RULE}←  ${tabs}  ✔ Submit  →`);
    const cur = (row: number) => (this.cursor === row ? "❯" : " ");
    if (this.reviewing) {
      lines.push("", "Review your answers", "  ");
      this.questions.forEach((q, at) => lines.push(` ● ${q.question}`, `   → ${this.answerOf(at)}`));
      lines.push("", "Ready to submit your answers?", "", `${cur(0)} 1. Submit answers  2. Cancel`);
      return lines.join("\n");
    }
    const q = this.questions[this.tab];
    const typeRow = q.options.length;
    lines.push("", q.question, "");
    q.options.forEach((option, at) => {
      const box = q.multiSelect ? `[${this.ticked[this.tab].has(at) ? "✔" : " "}] ` : "";
      lines.push(`${cur(at)} ${at + 1}. ${box}${option.label}`, `     ${option.description}`);
    });
    const words = this.typed[this.tab];
    if (q.multiSelect) {
      lines.push(`${cur(typeRow)} ${typeRow + 1}. [${this.typedTicked[this.tab] ? "✔" : " "}] ${words || "Type something"}`);
      lines.push(`${cur(typeRow + 1)}    ${this.tab === this.questions.length - 1 ? "Submit" : "Next"}`);
      lines.push(`${RULE}  ${typeRow + 2}. Chat about this`);
    } else {
      lines.push(`${cur(typeRow)} ${typeRow + 1}. ${words || "Type something."}`, RULE, `  ${typeRow + 2}. Chat about this`);
    }
    lines.push("", "Enter to select · Tab/Arrow keys to navigate · Esc to cancel", `${"─".repeat(56)} Ava (Shop) ─`);
    return lines.join("\n");
  }
}

function ioFor(dialog: Dialog, typed: AnswerStep[] = []): AnswerIo {
  return {
    read: async () => dialog.screen(),
    type: async (step) => {
      typed.push(step);
      if (step.text) dialog.paste(step.text);
      for (const key of step.keys) dialog.key(key);
    },
    sleep: async () => undefined,
  };
}

async function answer(questions: ChatQuestion[], answers: unknown, dialog = new Dialog(questions)) {
  await typeAnswers(ioFor(dialog), questions, choicesOf(questions, answers));
  return dialog.submitted;
}

describe("the keys for one question", () => {
  it("a single-select option: down to it, Enter", () => {
    expect(stepsFor(COLOR, { picked: [1] })).toEqual([{ keys: ["down", "enter"] }]);
    expect(stepsFor(COLOR, { picked: [0] })).toEqual([{ keys: ["enter"] }]);
  });

  it("a single-select question's own words: down to Type something, paste, Enter", () => {
    expect(stepsFor(PET, { picked: [], other: "A hamster" })).toEqual([
      { keys: ["down", "down"] },
      { text: "A hamster", keys: ["enter"] },
    ]);
  });

  it("multi-select: Enter on each option, then down to Next and Enter", () => {
    expect(stepsFor(FRUITS, { picked: [0, 2] })).toEqual([
      { keys: ["enter", "down", "down", "enter", "down", "down", "down", "enter"] },
    ]);
    expect(stepsFor(FRUITS, { picked: [3] })).toEqual([{ keys: ["down", "down", "down", "enter", "down", "down", "enter"] }]);
  });

  it("multi-select with the person's own words: they tick Type something as they land", () => {
    expect(stepsFor(FRUITS, { picked: [0, 2], other: "Mango, ripe" })).toEqual([
      { keys: ["enter", "down", "down", "enter", "down", "down"] },
      { text: "Mango, ripe", keys: ["down", "enter"] },
    ]);
    expect(stepsFor(FRUITS, { picked: [], other: "Kiwi" })).toEqual([
      { keys: ["down", "down", "down", "down"] },
      { text: "Kiwi", keys: ["down", "enter"] },
    ]);
  });
});

describe("answering the dialog", () => {
  it("several questions, multi-select and own words: each tab, then the review's Submit", async () => {
    const dialog = new Dialog([COLOR, FRUITS, PET]);
    const typed: AnswerStep[] = [];
    await typeAnswers(
      ioFor(dialog, typed),
      [COLOR, FRUITS, PET],
      choicesOf(
        [COLOR, FRUITS, PET],
        [{ selected: ["Green"] }, { selected: ["Cherry", "Apple"], other: "Mango, ripe" }, { selected: [], other: 'A "small" hamster' }],
      ),
    );
    // What Claude Code 2.1.312 recorded for these keys in a real session.
    expect(dialog.submitted).toEqual({
      "Which color do you prefer?": "Green",
      "Which fruits do you like?": 'Apple, Cherry, "Mango, ripe"',
      "Which pet would you adopt?": 'A "small" hamster',
    });
    expect(typed).toEqual([
      { keys: ["down", "enter"] },
      { keys: ["enter", "down", "down", "enter", "down", "down"] },
      { text: "Mango, ripe", keys: ["down", "enter"] },
      { keys: ["down", "down"] },
      { text: 'A "small" hamster', keys: ["enter"] },
      { keys: ["enter"] },
    ]);
  });

  it("a lone single-select question: its Enter submits, once the screen shows the choice", async () => {
    const dialog = new Dialog([SIZE]);
    const typed: AnswerStep[] = [];
    await typeAnswers(ioFor(dialog, typed), [SIZE], choicesOf([SIZE], [{ selected: ["Large"] }]));
    expect(dialog.submitted).toEqual({ "Which size?": "Large" });
    expect(typed).toEqual([{ keys: ["down"] }, { keys: ["enter"] }]);

    expect(await answer([SIZE], [{ selected: ["Small"] }])).toEqual({ "Which size?": "Small" });
    expect(await answer([SIZE], [{ selected: [], other: "Medium, please" }])).toEqual({ "Which size?": "Medium, please" });
  });

  it("a lone multi-select question goes through the review", async () => {
    expect(await answer([TOOLS], [{ selected: ["Drill", "Hammer"] }])).toEqual({ "Which tools?": "Hammer, Drill" });
    expect(await answer([TOOLS], [{ selected: ["Saw"], other: "A level" }])).toEqual({ "Which tools?": "Saw, A level" });
  });

  it("finds the dialog below its tab bar, not a todo or a prompt naming the same words", () => {
    const lines = dialogOf(new Dialog([COLOR, PET]).screen(), [COLOR, PET]);
    expect(lines?.[1]).toBe("Which color do you prefer?");
    expect(untouched(lines!, COLOR)).toBe(true);
    expect(dialogOf("❯ Which color do you prefer?\n  ☐ Something else", [COLOR])).toBeNull();
  });

  it("types nothing into a dialog someone at the PC has started on", async () => {
    for (const touch of [["down"], ["enter"], ["down", "down", "down"]]) {
      const dialog = new Dialog([FRUITS, PET]);
      for (const key of touch) dialog.key(key);
      const before = dialog.keys.length;
      await expect(answer([FRUITS, PET], [{ selected: ["Apple"] }, { selected: ["Cat"] }], dialog)).rejects.toThrow(
        NOT_ON_SCREEN,
      );
      expect(dialog.keys.length).toBe(before);
    }
    // Already past the first question.
    const dialog = new Dialog([COLOR, PET]);
    dialog.key("enter");
    await expect(answer([COLOR, PET], [{ selected: ["Red"] }, { selected: ["Cat"] }], dialog)).rejects.toThrow(
      NOT_ON_SCREEN,
    );
  });

  it("types nothing when the dialog is gone from the screen, or shows other options", async () => {
    const answered = new Dialog([COLOR]);
    answered.key("enter");
    expect(answered.submitted).toEqual({ "Which color do you prefer?": "Red" });
    await expect(answer([COLOR], [{ selected: ["Blue"] }], answered)).rejects.toThrow(NOT_ON_SCREEN);
    expect(answered.keys).toEqual(["enter"]);

    const other = new Dialog([question("Color", "Which color do you prefer?", ["Red", "Blue", "Green"])]);
    await expect(answer([COLOR], [{ selected: ["Blue"] }], other)).rejects.toThrow(NOT_ON_SCREEN);
    expect(other.keys).toEqual([]);
  });

  it("submits nothing when a key was lost on the way", async () => {
    // The Enter that ticks Cherry: the review shows Apple alone.
    const lost = new Dialog([COLOR, FRUITS], [5]);
    await expect(
      answer([COLOR, FRUITS], [{ selected: ["Green"] }, { selected: ["Apple", "Cherry"] }], lost),
    ).rejects.toThrow(NOT_REVIEWED);
    expect(lost.submitted).toBeNull();

    // The Enter that moves past the first question: the second never comes up.
    const stuck = new Dialog([COLOR, PET], [1]);
    await expect(answer([COLOR, PET], [{ selected: ["Green"] }, { selected: ["Dog"] }], stuck)).rejects.toThrow(
      lostAt(1),
    );
    expect(stuck.submitted).toBeNull();

    // A lone question whose cursor did not reach the answer.
    const short = new Dialog([SIZE], [0]);
    await expect(answer([SIZE], [{ selected: ["Large"] }], short)).rejects.toThrow(NOT_REVIEWED);
    expect(short.submitted).toBeNull();
  });
});

describe("the phone's answers", () => {
  it("are option labels exactly as given, and the person's own words cleaned of line breaks", () => {
    expect(
      choicesOf([COLOR, FRUITS], [{ selected: ["Blue"] }, { selected: ["Date", "Apple", "Date"], other: " Kiwi\nand\tlime\u0007 " }]),
    ).toEqual([{ picked: [2] }, { picked: [0, 3], other: "Kiwi and lime" }]);
    expect(choicesOf([PET], [{ other: "Fish" }])).toEqual([{ picked: [], other: "Fish" }]);
  });

  it("refuses a label that is not among the options", () => {
    expect(() => choicesOf([COLOR], [{ selected: ["Purple"] }])).toThrow('"Purple" is not one of the options of "Color".');
    expect(() => choicesOf([COLOR, PET], [{ selected: ["Red"] }, { selected: ["cat"] }])).toThrow(
      '"cat" is not one of the options of Question 2 ("Pet").',
    );
  });

  it("refuses a single-select question given two answers, and a question given none", () => {
    expect(() => choicesOf([COLOR], [{ selected: ["Red", "Blue"] }])).toThrow("takes one answer");
    expect(() => choicesOf([COLOR], [{ selected: ["Red"], other: "Teal" }])).toThrow("takes one answer");
    expect(() => choicesOf([FRUITS], [{ selected: [], other: "  " }])).toThrow('"Fruits" has no answer.');
  });

  it("refuses answers that do not match the questions one for one", () => {
    expect(() => choicesOf([COLOR, PET], [{ selected: ["Red"] }])).toThrow("one answer per question: 2");
    expect(() => choicesOf([COLOR], "Red")).toThrow("Give one answer");
    expect(() => choicesOf([COLOR], [{ selected: "Red" }])).toThrow('"selected" is a list');
    expect(() => choicesOf([COLOR], [{ other: 3 }])).toThrow('"other" is the person\'s own words');
  });
});

describe("which question the pane waits on", () => {
  const asked = (toolUseId: string, answer?: ChatAnswer[]): ChatItem => ({
    id: toolUseId,
    at: 1,
    kind: "question",
    toolUseId,
    questions: [COLOR],
    ...(answer ? { answer } : {}),
  });

  it("is the pending one, by its toolUseId", () => {
    expect(pendingQuestions({ toolUseId: "q2", questions: [COLOR] }, [asked("q2")], "q2")).toEqual([COLOR]);
  });

  it("refuses a question already answered, dismissed, or replaced by another", () => {
    const answered = [asked("q1", [{ selected: ["Red"] }])];
    expect(() => pendingQuestions(null, answered, "q1")).toThrow("This question was already answered.");
    expect(() => pendingQuestions({ toolUseId: "q2", questions: [PET] }, answered, "q1")).toThrow("already answered");
    expect(() => pendingQuestions(null, [asked("q1")], "q1")).toThrow("no longer waits on this question");
    expect(() => pendingQuestions({ toolUseId: "q2", questions: [PET] }, [], "q1")).toThrow(
      "now waits on another question",
    );
    expect(() => pendingQuestions(null, [], "q1")).toThrow("is not waiting on this question");
  });
});

describe("remote_answer", () => {
  const SEQUENCES: Record<string, string> = { "\r": "enter", "\x1b[B": "down", "\x1b[A": "up", "\x1b": "escape" };

  /** A window with one Claude Code pane, "Ava", showing `dialog`, and its transcript. */
  function rig(dialog: Dialog, page: Partial<ChatPage>, agent = "claude") {
    const ava = { id: "a", name: "Ava", agent, sessionId: "s1", cwd: null, shellId: null, title: null };
    const shop = { id: "shop", name: "Shop", cwd: "C:\\dev\\shop", extraRoots: [], panes: [ava], themeId: null, shellId: null, tree: null, savedCommands: [], folderId: null };
    const state = { workspaces: [shop], settings: { ...DEFAULT_SETTINGS } } as unknown as AppState;
    const deps = {
      snapshot: () => state,
      enabled: () => false,
      touch: () => undefined,
      isOpen: () => true,
      asleep: () => false,
      openWorkspace: () => undefined,
      terminal: async () => ({
        paste: (text: string) => dialog.paste(text),
        read: (n: number) => dialog.screen().split("\n").slice(-n),
        applicationCursor: false,
      }),
      write: async (_: string, data: string) => dialog.key(SEQUENCES[data] ?? data),
      sleep: async () => undefined,
    } as unknown as ControlDeps;
    const reads: unknown[] = [];
    const chat = async (args: unknown) => {
      reads.push(args);
      return { items: [], older: false, cursor: null, question: null, openCall: null, ...page };
    };
    const run = (args: Record<string, unknown>) => runRemote("remote_answer", { pane: "Ava", ...args }, deps, undefined, chat);
    return { run, reads };
  }

  it("types the answers into the pane and answers {answered: true}", async () => {
    const dialog = new Dialog([COLOR, FRUITS, PET]);
    const { run, reads } = rig(dialog, { question: { toolUseId: "q1", questions: [COLOR, FRUITS, PET] } });
    const answers = [{ selected: ["Blue"] }, { selected: ["Banana", "Date"] }, { selected: ["Dog"] }];
    expect(await run({ toolUseId: "q1", answers })).toEqual({ answered: true });
    expect(dialog.submitted).toEqual({
      "Which color do you prefer?": "Blue",
      "Which fruits do you like?": "Banana, Date",
      "Which pet would you adopt?": "Dog",
    });
    expect(reads).toEqual([{ sessionId: "s1", cwd: "C:\\dev\\shop" }]);
  });

  it("refuses a stale phone screen, an answered question, a wrong label, typing nothing", async () => {
    const answered: ChatItem = { id: "q0", at: 1, kind: "question", toolUseId: "q0", questions: [COLOR], answer: [{ selected: ["Red"] }] };
    const cases: [Partial<ChatPage>, Record<string, unknown>, string][] = [
      [{ question: { toolUseId: "q1", questions: [COLOR] } }, { toolUseId: "q0", answers: [{ selected: ["Red"] }] }, "now waits on another question"],
      [{ items: [answered] }, { toolUseId: "q0", answers: [{ selected: ["Blue"] }] }, "already answered"],
      [{ question: { toolUseId: "q1", questions: [COLOR] } }, { toolUseId: "q1", answers: [{ selected: ["Teal"] }] }, '"Teal" is not one of the options'],
      [{ question: { toolUseId: "q1", questions: [COLOR] } }, { answers: [{ selected: ["Red"] }] }, 'Name the question: its "toolUseId"'],
    ];
    for (const [page, args, error] of cases) {
      const dialog = new Dialog([COLOR]);
      await expect(rig(dialog, page).run(args)).rejects.toThrow(error);
      expect(dialog.keys).toEqual([]);
    }
  });

  it("answers the question the pane's mod reports, which the transcript does not have yet", async () => {
    const dialog = new Dialog([COLOR, PET]);
    const { run } = rig(dialog, {});
    noteBridgeEvent("a", { kind: "tool", phase: "start", toolUseId: "q9", tool: "AskUserQuestion", input: { questions: [COLOR, PET] } });
    try {
      await expect(run({ toolUseId: "q8", answers: [{ selected: ["Red"] }, { selected: ["Cat"] }] })).rejects.toThrow(
        "now waits on another question",
      );
      expect(dialog.keys).toEqual([]);
      expect(await run({ toolUseId: "q9", answers: [{ selected: ["Red"] }, { other: "A parrot" }] })).toEqual({
        answered: true,
      });
      expect(dialog.submitted).toEqual({ "Which color do you prefer?": "Red", "Which pet would you adopt?": "A parrot" });
    } finally {
      noteBridgeEvent("a", { kind: "tool", phase: "end", toolUseId: "q9" });
    }
    await expect(run({ toolUseId: "q9", answers: [{ selected: ["Red"] }, { selected: ["Cat"] }] })).rejects.toThrow(
      "not waiting on this question",
    );
  });

  it("refuses when the transcript still waits but the PC's screen no longer shows the question", async () => {
    const dialog = new Dialog([COLOR]);
    dialog.key("down");
    dialog.key("enter");
    const { run } = rig(dialog, { question: { toolUseId: "q1", questions: [COLOR] } });
    await expect(run({ toolUseId: "q1", answers: [{ selected: ["Red"] }] })).rejects.toThrow(NOT_ON_SCREEN);
    expect(dialog.submitted).toEqual({ "Which color do you prefer?": "Green" });
  });

  it("refuses a pane without a Claude Code question, and a second answer while one is typed", async () => {
    const page = { question: { toolUseId: "q1", questions: [COLOR] } };
    await expect(rig(new Dialog([COLOR]), page, "codex").run({ toolUseId: "q1", answers: [] })).rejects.toThrow(
      '"Ava" has no Claude Code question to answer.',
    );

    const dialog = new Dialog([COLOR]);
    const { run } = rig(dialog, page);
    const first = run({ toolUseId: "q1", answers: [{ selected: ["Blue"] }] });
    const second = run({ toolUseId: "q1", answers: [{ selected: ["Red"] }] });
    await expect(second).rejects.toThrow('An answer is being typed into "Ava" already.');
    await expect(first).resolves.toEqual({ answered: true });
    expect(dialog.submitted).toEqual({ "Which color do you prefer?": "Blue" });
  });
});
