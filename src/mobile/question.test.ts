import { describe, expect, it } from "vitest";
import type { ChatQuestion } from "./api";
import { CORA_QUESTIONS } from "./mockChat";
import {
  answersOf,
  canSend,
  choose,
  chooseOther,
  isAnswered,
  isLastStep,
  nextStep,
  previousStep,
  startDraft,
  typeOther,
  type Draft,
} from "./question";

const questions: ChatQuestion[] = CORA_QUESTIONS;

/** Runs the card's steps on a fresh draft. */
const play = (...steps: ((draft: Draft) => Draft)[]) => steps.reduce((draft, step) => step(draft), startDraft(questions));

describe("the question card", () => {
  it("starts on the first question, nothing chosen", () => {
    const draft = startDraft(questions);
    expect(draft.step).toBe(0);
    expect(draft.choices).toHaveLength(3);
    expect(isAnswered(draft.choices[0])).toBe(false);
    expect(canSend(draft)).toBe(false);
  });

  it("keeps one option of a single choice, the last tapped", () => {
    const draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => choose(d, questions, "Wait for the screenshots"),
    );
    expect(draft.choices[0].selected).toEqual(["Wait for the screenshots"]);
  });

  it("toggles the options of a multiple choice, in the options' order", () => {
    const draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Known gaps"),
      (d) => choose(d, questions, "Screenshots"),
      (d) => choose(d, questions, "Bundle size"),
      (d) => choose(d, questions, "Known gaps"),
    );
    expect(draft.step).toBe(1);
    expect(draft.choices[1].selected).toEqual(["Screenshots", "Bundle size"]);
  });

  it("takes Other… instead of an option for a single choice, and an option back instead of it", () => {
    let draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => chooseOther(d, questions),
    );
    expect(draft.choices[0]).toEqual({ selected: [], other: true, text: "" });
    // Other… without words is not an answer.
    expect(isAnswered(draft.choices[0])).toBe(false);
    draft = typeOther(draft, "After the review");
    expect(isAnswered(draft.choices[0])).toBe(true);
    draft = choose(draft, questions, "Submit now");
    expect(draft.choices[0]).toMatchObject({ selected: ["Submit now"], other: false });
    // Its words are kept, should it be chosen again.
    expect(chooseOther(draft, questions).choices[0]).toMatchObject({ selected: [], other: true, text: "After the review" });
  });

  it("takes Other… beside options for a multiple choice", () => {
    const draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Screenshots"),
      (d) => chooseOther(d, questions),
    );
    expect(isAnswered(draft.choices[1])).toBe(false);
    const typed = typeOther(draft, "The bundle's treemap");
    expect(typed.choices[1]).toEqual({ selected: ["Screenshots"], other: true, text: "The bundle's treemap" });
    expect(isAnswered(typed.choices[1])).toBe(true);
    expect(chooseOther(typed, questions).choices[1].other).toBe(false);
  });

  it("moves on only once a question is answered, and back keeps what was chosen", () => {
    let draft = startDraft(questions);
    expect(nextStep(draft, questions)).toBe(draft);
    draft = nextStep(choose(draft, questions, "Submit now"), questions);
    expect(draft.step).toBe(1);
    draft = previousStep(draft);
    expect(draft.step).toBe(0);
    expect(draft.choices[0].selected).toEqual(["Submit now"]);
    expect(previousStep(draft)).toBe(draft);
  });

  it("stays on the last question, where it sends", () => {
    const draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Test summary"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "The week away"),
    );
    expect(isLastStep(draft, questions)).toBe(true);
    expect(nextStep(draft, questions)).toBe(draft);
    expect(canSend(draft)).toBe(true);
  });

  it("sends one answer per question, in their order, the typed words trimmed", () => {
    const draft = play(
      (d) => chooseOther(d, questions),
      (d) => typeOther(d, "  After the review  "),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Test summary"),
      (d) => choose(d, questions, "Screenshots"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Push notifications"),
    );
    expect(answersOf(draft)).toEqual([
      { selected: [], other: "After the review" },
      { selected: ["Screenshots", "Test summary"] },
      { selected: ["Push notifications"] },
    ]);
  });

  it("leaves Other…'s words out once it is unticked", () => {
    const draft = play(
      (d) => choose(d, questions, "Submit now"),
      (d) => nextStep(d, questions),
      (d) => choose(d, questions, "Screenshots"),
      (d) => chooseOther(d, questions),
      (d) => typeOther(d, "treemap"),
      (d) => chooseOther(d, questions),
    );
    expect(answersOf(draft)[1]).toEqual({ selected: ["Screenshots"] });
  });
});
