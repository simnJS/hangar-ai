import { useId, useRef, useState, type ReactNode } from "react";
import { Icon } from "./Icon";
import { StateGlyph } from "./StateGlyph";
import { ToolbarMenu, type MenuCloseReason } from "./ToolbarMenu";
import { useT } from "../i18n";
import { useDelivering } from "../lib/deliveryRuns";
import { KINDS, latestNote, type NeedsYouItem, type NeedsYouKind } from "../lib/needsYou";
import { useShortcutTitle } from "../lib/useShortcuts";
import type { TranslateKey } from "../i18n";
import type { Workspace } from "../types";

export type LandItem = Extract<NeedsYouItem, { kind: "land" }>;

interface Props {
  items: NeedsYouItem[];
  /** For the orchestrator a question is answered to, by name. */
  workspaces: Workspace[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Brings a pane forward, switching workspace if it has to. */
  onGoToPane: (workspaceId: string, paneId: string) => void;
  onOpenBoard: (workspaceId: string) => void;
  /** Lands a delivery; rejects with what went wrong. */
  onLand: (item: LandItem) => Promise<void>;
}

const HEADINGS: Record<NeedsYouKind, TranslateKey> = {
  waiting: "needsYou.waiting",
  question: "needsYou.questions",
  land: "needsYou.lands",
};

/**
 * « Besoin de toi » in the toolbar: a count of what waits for the person, and
 * the list behind it (lib/needsYou). The button is only there while the list
 * has something in it.
 *
 * The list hangs off the button like the toolbar's menus, and keeps Tab
 * among its own buttons: each row says what waits and where, and offers the
 * one thing to do about it — go to the agent, answer the orchestrator, merge
 * the delivery — plus the board, where the task says the rest.
 */
export function NeedsYou({
  items,
  workspaces,
  open,
  onOpenChange,
  onGoToPane,
  onOpenBoard,
  onLand,
}: Props) {
  const t = useT();
  const withKeys = useShortcutTitle();
  const buttonRef = useRef<HTMLButtonElement>(null);
  const titleId = useId();
  /** What « Fusionner » last failed with, by item. */
  const [failures, setFailures] = useState<Record<string, string>>({});

  if (!items.length) return null;

  const label = t("needsYou.button", { n: items.length });

  const close = (reason: MenuCloseReason) => {
    onOpenChange(false);
    if (reason === "escape" || reason === "tab") buttonRef.current?.focus();
  };

  /** The row's action runs once the list is gone, so it gets the keyboard. */
  const leave = (action: () => void) => () => {
    onOpenChange(false);
    action();
  };

  const land = (item: LandItem) => {
    setFailures((current) => {
      const next = { ...current };
      delete next[item.key];
      return next;
    });
    onLand(item).catch((err: unknown) =>
      setFailures((current) => ({
        ...current,
        [item.key]: err instanceof Error ? err.message : String(err),
      })),
    );
  };

  const orchestratorName = (item: Extract<NeedsYouItem, { kind: "question" }>) =>
    workspaces
      .find((ws) => ws.id === item.workspaceId)
      ?.panes.find((pane) => pane.id === item.orchestratorId)?.name ?? null;

  /** The first row's first action takes the keyboard when the list opens. */
  let first = true;
  const takeFocus = () => {
    const take = first;
    first = false;
    return take;
  };

  function row(item: NeedsYouItem) {
    const boardButton = item.kind !== "waiting" && (
      <button
        type="button"
        className="needs__icon-btn"
        title={t("needsYou.board")}
        aria-label={t("needsYou.board")}
        onClick={leave(() => onOpenBoard(item.workspaceId))}
      >
        <Icon name="board" size={13} />
      </button>
    );

    if (item.kind === "waiting") {
      return (
        <li key={item.key} className="needs__row">
          <span className="needs__mark needs__mark--waiting" aria-hidden="true">
            <StateGlyph state="waiting" size={7} />
          </span>
          <span className="needs__text">
            <span className="needs__what">{item.paneName}</span>
            <span className="needs__meta">
              {item.workspaceName} · {t("needsYou.waitingDetail")}
            </span>
          </span>
          <span className="needs__actions">
            <button
              type="button"
              className="btn btn--tiny"
              autoFocus={takeFocus()}
              aria-label={t("needsYou.goTo", { name: item.paneName })}
              onClick={leave(() => onGoToPane(item.workspaceId, item.paneId))}
            >
              {t("needsYou.go")}
            </button>
          </span>
        </li>
      );
    }

    // Where, and who did the work; then what the task's last comment says —
    // the question itself, or the review's verdict — on two lines at most.
    const note = latestNote(item.task);
    const meta = (
      <>
        <span className="needs__meta">
          {item.workspaceName}
          {item.task.assignee && ` · ${item.task.assignee}`}
        </span>
        {note && (
          <span className="needs__note" title={note}>
            {note}
          </span>
        )}
      </>
    );

    if (item.kind === "question") {
      const name = orchestratorName(item);
      return (
        <li key={item.key} className="needs__row">
          <span className="needs__mark needs__mark--question" aria-hidden="true">
            <Icon name="flag" size={13} />
          </span>
          <span className="needs__text">
            <span className="needs__what">{item.task.title}</span>
            {meta}
          </span>
          <span className="needs__actions">
            {name && item.orchestratorId ? (
              <button
                type="button"
                className="btn btn--tiny"
                autoFocus={takeFocus()}
                aria-label={t("needsYou.answerIn", { name })}
                title={t("needsYou.answerIn", { name })}
                onClick={leave(() => onGoToPane(item.workspaceId, item.orchestratorId!))}
              >
                {t("needsYou.answer")}
              </button>
            ) : null}
            {boardButton}
          </span>
        </li>
      );
    }

    return (
      <LandRow
        key={item.key}
        item={item}
        meta={meta}
        failure={failures[item.key]}
        focus={takeFocus()}
        onLand={land}
        board={boardButton}
      />
    );
  }

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        className={`needs-btn ${open ? "is-open" : ""}`}
        onClick={() => onOpenChange(!open)}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-label={label}
        title={withKeys(label, "view.needsYou")}
      >
        <Icon name="flag" size={14} />
        <span className="needs-btn__label">{t("needsYou.title")}</span>
        <span className="needs-btn__count">{items.length}</span>
      </button>

      {open && (
        <ToolbarMenu
          anchorRef={buttonRef}
          role="dialog"
          labelledBy={titleId}
          className="needs"
          onClose={close}
        >
          <header className="needs__head">
            <h2 className="needs__title" id={titleId}>
              {t("needsYou.title")}
            </h2>
            <span className="needs__total" aria-hidden="true">
              {items.length}
            </span>
          </header>
          <div className="needs__body">
            {KINDS.map((kind) => {
              const group = items.filter((item) => item.kind === kind);
              if (!group.length) return null;
              const headingId = `${titleId}-${kind}`;
              return (
                <section key={kind} className="needs__group" aria-labelledby={headingId}>
                  <h3 className="needs__group-title" id={headingId}>
                    {t(HEADINGS[kind])}
                    <span className="needs__group-count">{group.length}</span>
                  </h3>
                  <ul className="needs__list">{group.map(row)}</ul>
                </section>
              );
            })}
          </div>
        </ToolbarMenu>
      )}
    </>
  );
}

function LandRow({
  item,
  meta,
  failure,
  focus,
  onLand,
  board,
}: {
  item: LandItem;
  meta: ReactNode;
  failure: string | undefined;
  focus: boolean;
  onLand: (item: LandItem) => void;
  board: ReactNode;
}) {
  const t = useT();
  // Running, wherever it was started from: a second click would land twice.
  const landing = useDelivering(item.workspaceId, item.task.id);
  return (
    <li className="needs__row">
      <span className="needs__mark needs__mark--land" aria-hidden="true">
        <Icon name="merge" size={13} />
      </span>
      <span className="needs__text">
        <span className="needs__what">{item.task.title}</span>
        {meta}
        {failure && (
          <span className="needs__error" role="alert">
            <Icon name="alert" size={11} />
            {t("needsYou.landFailed", { error: failure })}
          </span>
        )}
      </span>
      <span className="needs__actions">
        <button
          type="button"
          className="btn btn--tiny btn--primary"
          autoFocus={focus}
          disabled={landing}
          aria-busy={landing || undefined}
          title={t("needsYou.landHint")}
          onClick={() => onLand(item)}
        >
          <Icon name="merge" size={11} />
          {t(landing ? "needsYou.landing" : "needsYou.land")}
        </button>
        {board}
      </span>
    </li>
  );
}
