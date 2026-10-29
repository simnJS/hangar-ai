import { memo, useCallback, useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ApiError, type ActionState, type Chat, type ChatAnswer, type ChatItem, type Pending } from "../api";
import {
  actionKind,
  actionLine,
  actionTitle,
  afterNewItems,
  afterScroll,
  anchoredScrollTop,
  chatRows,
  chipOf,
  diffLine,
  diffLog,
  mergeLatest,
  mergeOlder,
  noteFolds,
  noticeText,
  PAGE_LIMIT,
  pathParts,
  pendingKey,
  POLL_LIMIT,
  shouldLoadOlder,
  showsPending,
  stillSending,
  visibleActions,
  waitsElsewhere,
  ANSWER_SETTLES_MS,
  type ActionItem,
  type ActionKind,
  type Answered,
  type ChatLog,
  type ChatRow,
  type Follow,
  type Outgoing,
  type QuestionItem,
  type UserItem,
} from "../chat";
import { Icon, Logo, type IconName } from "../components/Icon";
import { Markdown } from "../components/Markdown";
import { errorText } from "../errors";
import { clock, dayHeading } from "../format";
import { usePolling } from "../hooks";
import { locale, t, tn } from "../i18n";
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
} from "../question";
import { useRemoteContext } from "../remote";
import { Composer, Console, PaneHeader, useStatus, type ConsoleProps } from "./Console";

/**
 * A Claude Code pane as a chat: the person's messages on the right, Claude's
 * answers on the left in markdown, its tool calls as one line each between
 * them, and what it waits for — a question, a permission — as a card at the
 * foot, answered with taps. Read every second while it is seen; the list
 * follows the newest message unless the person scrolled up to read, and the
 * page before loads as they reach the top.
 *
 * Its terminal stays one tap away, with the keys: the Terminal button opens
 * the Console, as a step the back gesture returns from. A pane with no chat,
 * no session yet, shows its terminal at once.
 */

type Head = Omit<Chat, "items" | "older" | "cursor">;
type Older = "idle" | "loading" | "failed";

const headOf = ({ items: _items, older: _older, cursor: _cursor, ...head }: Chat): Head => head;
const sameHead = (head: Head | null, page: Chat) => head !== null && JSON.stringify(head) === JSON.stringify(headOf(page));

const NO_ITEMS: ChatItem[] = [];

/**
 * The terminal, as a step of its own in the phone's history: the back
 * gesture, or the back button, returns to the chat.
 */
function useTerminalView() {
  const [open, setOpen] = useState(() => history.state?.terminal === true);
  useEffect(() => {
    const update = () => setOpen(history.state?.terminal === true);
    addEventListener("popstate", update);
    return () => removeEventListener("popstate", update);
  }, []);
  const show = useCallback(() => {
    history.pushState({ ...history.state, depth: (history.state?.depth ?? 0) + 1, terminal: true }, "");
    setOpen(true);
  }, []);
  const close = useCallback(() => {
    if (history.state?.terminal === true) history.back();
    else setOpen(false);
  }, []);
  return { open, show, close };
}

export function ChatPane(props: ConsoleProps) {
  const { client } = useRemoteContext();
  const terminal = useTerminalView();
  const [log, setLog] = useState<ChatLog | null>(null);
  const [head, setHead] = useState<Head | null>(null);
  const [gone, setGone] = useState(false);
  /** The PC has no chat to give: an older Hangar. */
  const [unsupported, setUnsupported] = useState(false);
  const [older, setOlder] = useState<Older>("idle");
  const logRef = useRef(log);
  logRef.current = log;
  const olderRef = useRef(older);
  olderRef.current = older;

  const poll = useCallback(async () => {
    try {
      const page = await client.chat(props.paneId, { limit: logRef.current ? POLL_LIMIT : PAGE_LIMIT });
      setHead((current) => (sameHead(current, page) ? current : headOf(page)));
      setLog((current) => mergeLatest(current, page).log);
      setGone(false);
    } catch (error) {
      if (!(error instanceof ApiError && error.kind === "refused")) return;
      if (error.status === 404) setUnsupported(true);
      else setGone(true);
    }
  }, [client, props.paneId]);

  // Without a chat, a slower look in case a session starts.
  usePolling(poll, head?.reason || unsupported ? 5_000 : 1_000, !terminal.open);

  const loadOlder = useCallback(async () => {
    const current = logRef.current;
    if (!current?.older || current.cursor === null || olderRef.current === "loading") return;
    const before = current.cursor;
    olderRef.current = "loading";
    setOlder("loading");
    try {
      const page = await client.chat(props.paneId, { before, limit: PAGE_LIMIT });
      setLog((latest) => (latest ? mergeOlder(latest, page, before).log : latest));
      setOlder("idle");
    } catch {
      setOlder("failed");
    }
  }, [client, props.paneId]);

  if (terminal.open || head?.reason || unsupported) {
    return (
      <Console
        {...props}
        activity={head?.activity ?? props.activity}
        {...(terminal.open ? { onBack: terminal.close, backLabel: t("chat.backToChat") } : {})}
      />
    );
  }
  return (
    <ChatView
      {...props}
      log={log}
      head={head}
      gone={gone}
      older={older}
      loadOlder={() => void loadOlder()}
      openTerminal={terminal.show}
      refresh={poll}
    />
  );
}

interface ChatViewProps extends ConsoleProps {
  log: ChatLog | null;
  head: Head | null;
  gone: boolean;
  older: Older;
  loadOlder: () => void;
  openTerminal: () => void;
  refresh: () => Promise<void>;
}

function ChatView(props: ChatViewProps) {
  const { client } = useRemoteContext();
  const { log, head, gone, older, loadOlder } = props;
  const { status, setStatus, act } = useStatus();
  const activity = head ? head.activity : props.activity;
  const pending = head?.pending ?? null;
  const items = log?.items ?? NO_ITEMS;
  const rows = useMemo(() => chatRows(items, pending), [items, pending]);

  /* What the person answered, for the card to step aside while the PC catches up. */
  const [answered, setAnswered] = useState<Answered | null>(null);
  const [, redraw] = useState(0);
  useEffect(() => {
    if (!answered) return;
    const timer = setTimeout(() => redraw((n) => n + 1), ANSWER_SETTLES_MS + 100);
    return () => clearTimeout(timer);
  }, [answered]);
  const now = Date.now();
  const card = showsPending(pending, answered, now) ? pending : null;
  const elsewhere = log !== null && waitsElsewhere(activity, pending, answered, now);
  const working = activity === "working" && !card;

  /* Messages sent, until the transcript holds them. */
  const [outbox, setOutbox] = useState<Outgoing[]>([]);
  const itemsRef = useRef(items);
  itemsRef.current = items;
  useEffect(() => setOutbox((current) => stillSending(current, items, Date.now())), [items]);

  /* Following the newest message. */
  const scroller = useRef<HTMLDivElement>(null);
  const content = useRef<HTMLDivElement>(null);
  const [follow, setFollow] = useState<Follow>({ following: true, unseen: 0 });
  const followRef = useRef(follow);
  const shown = useRef<ChatItem[]>(NO_ITEMS);
  /** The list's height as last laid out: what a page put above pushes it by. */
  const height = useRef(0);
  const olderRef = useRef({ log, older, loadOlder });
  olderRef.current = { log, older, loadOlder };

  const setFollowing = (next: Follow) => {
    if (next.following === followRef.current.following && next.unseen === followRef.current.unseen) return;
    followRef.current = next;
    setFollow(next);
  };

  const maybeLoadOlder = (element: HTMLElement) => {
    const { log, older, loadOlder } = olderRef.current;
    if (shouldLoadOlder(element, log, older !== "idle")) loadOlder();
  };

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const change = diffLog(shown.current, items);
    shown.current = items;
    if (change.reset) {
      element.scrollTop = element.scrollHeight;
      setFollowing({ following: true, unseen: 0 });
    } else {
      if (change.prepended) {
        element.scrollTop = anchoredScrollTop({ scrollTop: element.scrollTop, scrollHeight: height.current }, element.scrollHeight);
      }
      const next = afterNewItems(followRef.current, change.appended);
      if (next.scroll) element.scrollTop = element.scrollHeight;
      setFollowing(next);
    }
    height.current = element.scrollHeight;
    maybeLoadOlder(element);
  }, [items]);

  // What grows or shrinks — a card, an action opened, the keyboard — keeps
  // the bottom in view while the list follows.
  useEffect(() => {
    const element = scroller.current;
    const inner = content.current;
    if (!element || !inner || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      if (followRef.current.following) element.scrollTop = element.scrollHeight;
      height.current = element.scrollHeight;
    });
    observer.observe(element);
    observer.observe(inner);
    return () => observer.disconnect();
  }, []);

  const onScroll = () => {
    const element = scroller.current;
    if (!element) return;
    setFollowing(afterScroll(followRef.current, element));
    maybeLoadOlder(element);
  };

  const toEnd = () => {
    const element = scroller.current;
    if (element) element.scrollTop = element.scrollHeight;
    setFollowing({ following: true, unseen: 0 });
  };

  const chip = chipOf(follow, card);

  /* Answering. */
  const answerQuestion = async (toolUseId: string, answers: ChatAnswer[]) => {
    setAnswered({ key: `question:${toolUseId}`, at: Date.now() });
    try {
      await client.answer(props.paneId, toolUseId, answers);
    } catch (error) {
      setAnswered(null);
      throw error;
    }
    setStatus({ kind: "done", text: t("question.sent") });
    void props.refresh();
  };

  const answerPermission = async (allow: boolean) => {
    setAnswered({ key: pendingKey(pending) ?? "", at: Date.now() });
    try {
      await client.permission(props.paneId, allow);
    } catch (error) {
      setAnswered(null);
      throw error;
    }
    setStatus({ kind: "done", text: allow ? t("permission.allowed") : t("permission.denied") });
    void props.refresh();
  };

  const interrupt = async () => {
    if (await act(() => client.send({ pane: props.paneId, keys: ["escape"] }), t("chat.interrupted"))) void props.refresh();
  };

  /** The message shows at the foot as soon as it is sent, and goes again if it was not. */
  const send = async (text: string) => {
    const last = itemsRef.current[itemsRef.current.length - 1];
    const message = { id: `out-${Date.now()}`, text, at: Date.now(), after: last?.id ?? null };
    setOutbox((current) => [...current, message]);
    toEnd();
    try {
      await props.send(text);
    } catch (error) {
      setOutbox((current) => current.filter((candidate) => candidate !== message));
      throw error;
    }
    void props.refresh();
  };

  const asking = card !== null || elsewhere;

  return (
    <div className={`console chat ${asking ? "chat--asking" : ""} ${props.tabbed ? "chat--tabbed" : ""}`}>
      <PaneHeader
        {...props}
        activity={activity}
        aside={
          <>
            {props.app}
            <button type="button" className="head-action" onClick={props.openTerminal} aria-label={t("chat.terminalHint")}>
              <Icon name="terminal" />
              <span>{t("chat.terminal")}</span>
            </button>
          </>
        }
      />

      <div className="chat__body">
        <div className="chat__scroll" ref={scroller} onScroll={onScroll} aria-label={t("chat.label", { name: props.name })}>
          <div className="chat__content" ref={content}>
            {props.notice}
            {log === null ? (
              <p className="chat__note">{gone ? t("pane.gone") : t("chat.loading")}</p>
            ) : (
              <>
                <OlderEdge log={log} older={older} retry={loadOlder} />
                {!rows.length && !outbox.length && !asking && <p className="chat__note">{t("chat.empty")}</p>}
                <ol className="chat__list">
                  {rows.map((row) => (
                    <Row key={row.key} row={row} />
                  ))}
                  {outbox.map((message) => (
                    <li key={message.id} className="msg msg--you msg--sending">
                      <span className="sr-only">{t("chat.you")}</span>
                      <p className="bubble">{message.text}</p>
                      <span className="msg__meta">{t("chat.sending")}</span>
                    </li>
                  ))}
                </ol>
                {gone && <p className="chat__note">{t("pane.gone")}</p>}
                {card?.kind === "question" && (
                  <QuestionCard key={card.toolUseId} name={props.name} pending={card} send={answerQuestion} />
                )}
                {card?.kind === "permission" && (
                  <PermissionCard key={pendingKey(card)} name={props.name} pending={card} answer={answerPermission} />
                )}
                {elsewhere && (
                  <section className="ask-card is-waiting" aria-label={t("chat.waitsElsewhere", { name: props.name })}>
                    <p className="ask-card__who">{t("chat.waitsElsewhere", { name: props.name })}</p>
                    <div className="ask-card__actions">
                      <button type="button" className="btn btn--primary" onClick={props.openTerminal}>
                        <Icon name="terminal" />
                        {t("chat.openTerminal")}
                      </button>
                    </div>
                  </section>
                )}
              </>
            )}
          </div>
        </div>
        {chip && (
          <button type="button" className="to-end" onClick={toEnd}>
            <Icon name="arrowDown" />
            {chip.key === "chat.newMessages" ? tn("chat.newMessages", chip.n) : t(chip.key)}
          </button>
        )}
      </div>

      {asking ? (
        status && (
          <div className={`composer ${props.tabbed ? "composer--tabbed" : ""}`}>
            <StatusLine status={status} />
          </div>
        )
      ) : (
        <Composer
          send={send}
          placeholder={props.placeholder}
          workspaceId={props.workspaceId}
          tabbed={props.tabbed}
          status={status}
          setStatus={setStatus}
          act={act}
        >
          {working && (
            <div className="working" role="status">
              <span className="spinner" aria-hidden="true" />
              <span className="working__text">{t("chat.working")}</span>
              <button type="button" className="btn working__stop" onClick={() => void interrupt()} aria-label={t("chat.interruptHint")}>
                <Icon name="stop" size={16} />
                {t("chat.interrupt")}
              </button>
            </div>
          )}
        </Composer>
      )}
    </div>
  );
}

function StatusLine({ status }: { status: NonNullable<ReturnType<typeof useStatus>["status"]> }) {
  return (
    <p className={`status status--${status.kind}`} role={status.kind === "error" ? "alert" : "status"}>
      {status.kind === "done" && <Icon name="check" size={16} />}
      <span>{status.text}</span>
    </p>
  );
}

/** The top of what the phone holds: the page before loading, failed, or the conversation's start. */
function OlderEdge({ log, older, retry }: { log: ChatLog; older: Older; retry: () => void }) {
  if (older === "loading") {
    return (
      <p className="chat__edge" role="status">
        <span className="spinner" aria-hidden="true" />
        {t("chat.olderLoading")}
      </p>
    );
  }
  if (older === "failed") {
    return (
      <p className="chat__edge chat__edge--failed" role="alert">
        {t("chat.olderFailed")}
        <button type="button" className="btn" onClick={retry}>
          {t("error.retry")}
        </button>
      </p>
    );
  }
  if (!log.older && log.items.length) return <p className="chat__edge">{t("chat.start")}</p>;
  return null;
}

/** When something was said: its time today, its day before. */
function when(at: number): string {
  const day = dayHeading(at, Date.now(), locale);
  return new Date(at).toDateString() === new Date().toDateString() ? clock(at, locale) : `${day} ${clock(at, locale)}`;
}

const Row = memo(function Row({ row }: { row: ChatRow }) {
  switch (row.type) {
    case "time":
      return (
        <li className="chat-time">
          <time dateTime={new Date(row.at).toISOString()}>{when(row.at)}</time>
        </li>
      );
    case "user":
      return <UserMessage item={row.item} />;
    case "hangar":
      return <HangarNote text={row.text} />;
    case "assistant":
      return (
        <li className="msg msg--claude">
          <span className="sr-only">{t("chat.claude")}</span>
          <Markdown text={row.text} />
        </li>
      );
    case "actions":
      return (
        <li className="msg msg--actions">
          <Actions items={row.items} />
        </li>
      );
    case "question":
      return <AnsweredQuestion item={row.item} />;
    case "notice":
      return <li className="chat-notice">{noticeText(row.text, t)}</li>;
  }
});

function UserMessage({ item }: { item: UserItem }) {
  return (
    <li className="msg msg--you">
      <span className="sr-only">{t("chat.you")}</span>
      {item.text.trim() && <p className="bubble">{item.text}</p>}
      {item.images > 0 && (
        <span className="msg__meta">
          <Icon name="image" size={15} />
          {tn("chat.images", item.images)}
        </span>
      )}
    </li>
  );
}

/** What Hangar queued for the pane: a note of the system, folded when long. */
function HangarNote({ text }: { text: string }) {
  const folds = noteFolds(text);
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <li className="note">
      <p className="note__from">
        <Logo size={14} />
        {t("chat.hangar")}
      </p>
      <div id={id} className={`note__body ${folds && !open ? "note__body--folded" : ""}`}>
        <Markdown text={text} />
      </div>
      {folds && (
        <button type="button" className="note__more" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
          {open ? t("chat.showLess") : t("chat.showAll")}
          <Icon name="chevronDown" size={14} className={open ? "is-flipped" : undefined} />
        </button>
      )}
    </li>
  );
}

/* ── Actions ───────────────────────────────────────────────────── */

const ACTION_ICONS: Record<ActionKind, IconName> = {
  edit: "pencil",
  command: "terminal",
  read: "file",
  search: "search",
  other: "tool",
};

function Actions({ items }: { items: ActionItem[] }) {
  const [unfolded, setUnfolded] = useState(false);
  const { hidden, shown } = visibleActions(items, unfolded);
  return (
    <ul className="actions">
      {hidden > 0 && (
        <li>
          <button type="button" className="action__line action__more" onClick={() => setUnfolded(true)}>
            <Icon name="chevronDown" size={16} className="action__icon" />
            <span className="action__title">{tn("action.more", hidden)}</span>
          </button>
        </li>
      )}
      {shown.map((item) => (
        <ActionRow key={item.id} item={item} />
      ))}
    </ul>
  );
}

/** Running, a spinner; failed, a cross; done is the quiet state, said only to a screen reader. */
function StateMark({ state }: { state: ActionState }) {
  return (
    <>
      {state !== "done" && (
        <span className={`action__state action__state--${state}`} aria-hidden="true">
          {state === "running" ? <span className="spinner" /> : <Icon name="failed" size={16} />}
        </span>
      )}
      <span className="sr-only">{t(`action.${state}`)}</span>
    </>
  );
}

/** "Modifié session.rs src-tauri/src/remote": the file first, its folder dimmed after it. */
function ActionTitle({ title, kind }: { title: string; kind: ActionKind }) {
  const { before, subject, after } = actionLine(title, t);
  const path = kind === "edit" || kind === "read" ? pathParts(subject) : null;
  return (
    <span className="action__title">
      {before}
      {path ? (
        <>
          <span className="action__file">{path.base}</span>
          {after}
          <span className="action__dir"> {path.dir}</span>
        </>
      ) : (
        <>
          {subject}
          {after}
        </>
      )}
    </span>
  );
}

function ActionRow({ item }: { item: ActionItem }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const kind = actionKind(item.tool);
  const more = Boolean(item.detail || item.result?.text);
  const line = (
    <>
      <Icon name={ACTION_ICONS[kind]} size={16} className="action__icon" />
      <ActionTitle title={item.title} kind={kind} />
      <StateMark state={item.state} />
      {more && <Icon name="chevronDown" size={14} className={`action__chevron ${open ? "is-flipped" : ""}`} />}
    </>
  );
  return (
    <li className={`action action--${item.state}`}>
      {more ? (
        <button type="button" className="action__line" aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
          {line}
        </button>
      ) : (
        <div className="action__line">{line}</div>
      )}
      {open && (
        <div id={id} className="action__detail">
          {item.detail && (
            <>
              <p className="action__label">{t(`action.input.${kind}`)}</p>
              <Detail text={item.detail} diff={kind === "edit"} />
            </>
          )}
          {item.result?.text && (
            <>
              <p className="action__label">{item.result.ok ? t("action.result") : t("action.failure")}</p>
              <Detail text={item.result.text} failed={!item.result.ok} />
            </>
          )}
        </div>
      )}
    </li>
  );
}

/** A command, a diff or an output: monospace, scrolled inside itself. */
function Detail({ text, diff = false, failed = false }: { text: string; diff?: boolean; failed?: boolean }) {
  return (
    <pre className={`detail ${failed ? "detail--failed" : ""}`} tabIndex={0}>
      {diff
        ? text.split("\n").map((line, index) => (
            <span key={index} className={`detail__line ${diffLine(line) ? `detail__line--${diffLine(line)}` : ""}`}>
              {line || " "}
            </span>
          ))
        : text}
    </pre>
  );
}

/* ── Questions and permissions ─────────────────────────────────── */

/** A question answered earlier: what was asked, on Claude's side; what was chosen, on the person's. */
function AnsweredQuestion({ item }: { item: QuestionItem }) {
  return (
    <li className="asked">
      {item.questions.map((question, index) => {
        const answer = item.answer?.[index];
        return (
          <div key={index} className="asked__one">
            <p className="asked__question">
              {question.header && <span className="tag">{question.header}</span>}
              <span>{question.question}</span>
            </p>
            {answer && (answer.selected.length > 0 || answer.other) ? (
              <ul className="bubble asked__answer" aria-label={t("chat.you")}>
                {answer.selected.map((label) => (
                  <li key={label}>
                    <Icon name="check" size={15} />
                    {label}
                  </li>
                ))}
                {answer.other && (
                  <li className="asked__other">
                    <Icon name="pencil" size={15} />
                    {answer.other}
                  </li>
                )}
              </ul>
            ) : (
              <p className="asked__none">{t("question.unanswered")}</p>
            )}
          </div>
        );
      })}
    </li>
  );
}

function QuestionCard({
  name,
  pending,
  send,
}: {
  name: string;
  pending: Extract<Pending, { kind: "question" }>;
  send: (toolUseId: string, answers: ChatAnswer[]) => Promise<void>;
}) {
  const { questions, toolUseId } = pending;
  const [draft, setDraft] = useState(() => startDraft(questions));
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const group = useId();
  const question = questions[draft.step];
  const choice = draft.choices[draft.step];
  const last = isLastStep(draft, questions);
  if (!question || !choice) return null;
  const many = questions.length > 1;
  const multi = question.multiSelect;

  const submit = async () => {
    if (!canSend(draft) || sending) return;
    setSending(true);
    setError(null);
    try {
      await send(toolUseId, answersOf(draft));
    } catch (failure) {
      setError(errorText(failure));
      setSending(false);
    }
  };

  return (
    <section className="ask-card is-waiting" aria-label={tn("question.title", questions.length, { name })}>
      <p className="ask-card__who">{tn("question.title", questions.length, { name })}</p>
      <fieldset key={draft.step} className="question">
        <legend className="question__head">
          {(question.header || many) && (
            <span className="question__meta">
              {question.header && <span className="tag">{question.header}</span>}
              {many && <span className="question__step">{t("question.step", { n: draft.step + 1, total: questions.length })}</span>}
            </span>
          )}
          <span className="question__text">{question.question}</span>
          {multi && <span className="question__hint">{t("question.multi")}</span>}
        </legend>
        <div className="options">
          {question.options.map((option) => (
            <label key={option.label} className="option">
              <input
                type={multi ? "checkbox" : "radio"}
                name={`${group}-${draft.step}`}
                checked={choice.selected.includes(option.label)}
                onChange={() => setDraft((current) => choose(current, questions, option.label))}
              />
              <span className={`option__mark option__mark--${multi ? "box" : "dot"}`} aria-hidden="true" />
              <span className="option__text">
                <span className="option__label">{option.label}</span>
                {option.description && <span className="option__desc">{option.description}</span>}
              </span>
            </label>
          ))}
          <div className={`option option--other ${choice.other ? "is-open" : ""}`}>
            <label className="option__other">
              <input
                type={multi ? "checkbox" : "radio"}
                name={`${group}-${draft.step}`}
                checked={choice.other}
                onChange={() => {
                  setDraft((current) => chooseOther(current, questions));
                  requestAnimationFrame(() => field.current?.focus());
                }}
              />
              <span className={`option__mark option__mark--${multi ? "box" : "dot"}`} aria-hidden="true" />
              <span className="option__label">{t("question.other")}</span>
            </label>
            {choice.other && (
              <input
                ref={field}
                className="option__field"
                type="text"
                value={choice.text}
                placeholder={t("question.otherField")}
                aria-label={t("question.otherField")}
                enterKeyHint={last ? "send" : "next"}
                onChange={(event) => setDraft((current) => typeOther(current, event.target.value))}
                onKeyDown={(event) => {
                  if (event.key !== "Enter" || event.nativeEvent.isComposing) return;
                  event.preventDefault();
                  if (last) void submit();
                  else setDraft((current) => nextStep(current, questions));
                }}
              />
            )}
          </div>
        </div>
      </fieldset>
      {error && (
        <p className="status status--error" role="alert">
          <span>{error}</span>
        </p>
      )}
      <div className="ask-card__actions">
        {draft.step > 0 && (
          <button type="button" className="btn" onClick={() => setDraft(previousStep)}>
            <Icon name="chevronLeft" size={18} />
            {t("question.back")}
          </button>
        )}
        {last ? (
          <button type="button" className="btn btn--primary" disabled={!canSend(draft) || sending} onClick={() => void submit()}>
            {t("question.send")}
          </button>
        ) : (
          <button
            type="button"
            className="btn btn--primary"
            disabled={!isAnswered(choice)}
            onClick={() => setDraft((current) => nextStep(current, questions))}
          >
            {t("question.next")}
            <Icon name="chevronRight" size={18} />
          </button>
        )}
      </div>
    </section>
  );
}

function PermissionCard({
  name,
  pending,
  answer,
}: {
  name: string;
  pending: Extract<Pending, { kind: "permission" }>;
  answer: (allow: boolean) => Promise<void>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const decide = async (allow: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await answer(allow);
    } catch (failure) {
      setError(errorText(failure));
      setBusy(false);
    }
  };
  return (
    <section className="ask-card is-waiting" aria-label={t("permission.title", { name })}>
      <p className="ask-card__who">{t("permission.title", { name })}</p>
      <p className="ask-card__what">
        <span className="tag">{pending.tool}</span>
        <span>{actionTitle(pending.title, t)}</span>
      </p>
      {pending.detail && <Detail text={pending.detail} diff={actionKind(pending.tool) === "edit"} />}
      {error && (
        <p className="status status--error" role="alert">
          <span>{error}</span>
        </p>
      )}
      <div className="ask-card__actions ask-card__actions--both">
        <button type="button" className="btn btn--deny" disabled={busy} onClick={() => void decide(false)}>
          {t("permission.deny")}
        </button>
        <button type="button" className="btn btn--primary" disabled={busy} onClick={() => void decide(true)}>
          {t("permission.allow")}
        </button>
      </div>
    </section>
  );
}
