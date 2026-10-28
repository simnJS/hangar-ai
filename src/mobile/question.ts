import type { ChatAnswer, ChatQuestion } from "./api";

/**
 * The question card's state: one step per question, the options chosen in
 * each, and what was typed under "Other…". One option at a time for a
 * single choice, any number for a multiple one; "Other…" counts as one more,
 * and its text is kept when it is unticked and ticked again.
 */

export interface Choice {
  selected: string[];
  other: boolean;
  text: string;
}

export interface Draft {
  step: number;
  choices: Choice[];
}

export const startDraft = (questions: ChatQuestion[]): Draft => ({
  step: 0,
  choices: questions.map(() => ({ selected: [], other: false, text: "" })),
});

const update = (draft: Draft, change: (choice: Choice) => Choice): Draft => ({
  ...draft,
  choices: draft.choices.map((choice, index) => (index === draft.step ? change(choice) : choice)),
});

/** Taps an option of the current question. */
export function choose(draft: Draft, questions: ChatQuestion[], label: string): Draft {
  const question = questions[draft.step];
  if (!question) return draft;
  return update(draft, (choice) => {
    if (!question.multiSelect) return { ...choice, selected: [label], other: false };
    const selected = choice.selected.includes(label)
      ? choice.selected.filter((chosen) => chosen !== label)
      : // In the options' order, whatever the order they were tapped in.
        question.options.map((option) => option.label).filter((option) => option === label || choice.selected.includes(option));
    return { ...choice, selected };
  });
}

/** Taps "Other…" on the current question. */
export function chooseOther(draft: Draft, questions: ChatQuestion[]): Draft {
  const question = questions[draft.step];
  if (!question) return draft;
  return update(draft, (choice) =>
    question.multiSelect ? { ...choice, other: !choice.other } : { ...choice, selected: [], other: true },
  );
}

export const typeOther = (draft: Draft, text: string): Draft => update(draft, (choice) => ({ ...choice, text }));

/** Whether a question has its answer: an option, or "Other…" with words in it. */
export function isAnswered(choice: Choice | undefined): boolean {
  if (!choice) return false;
  if (choice.other && !choice.text.trim()) return false;
  return choice.selected.length > 0 || choice.other;
}

export const isLastStep = (draft: Draft, questions: ChatQuestion[]) => draft.step >= questions.length - 1;

/** On to the next question, once this one is answered. */
export function nextStep(draft: Draft, questions: ChatQuestion[]): Draft {
  if (isLastStep(draft, questions) || !isAnswered(draft.choices[draft.step])) return draft;
  return { ...draft, step: draft.step + 1 };
}

export const previousStep = (draft: Draft): Draft => (draft.step === 0 ? draft : { ...draft, step: draft.step - 1 });

/** Every question answered: what can be sent. */
export const canSend = (draft: Draft) => draft.choices.length > 0 && draft.choices.every(isAnswered);

/** What the card sends: one answer per question, in their order. */
export function answersOf(draft: Draft): ChatAnswer[] {
  return draft.choices.map((choice) =>
    choice.other ? { selected: choice.selected, other: choice.text.trim() } : { selected: choice.selected },
  );
}
