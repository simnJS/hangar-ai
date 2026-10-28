import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ApiError, type Activity, type Key, type Screen } from "../api";
import { StateLabel, useNavigation } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { errorText } from "../errors";
import { usePolling } from "../hooks";
import { t, type Key as Word } from "../i18n";
import { screenLines } from "../model";
import { useRemoteContext } from "../remote";
import type { Route } from "../route";

/**
 * A pane on the phone: its screen as text, following the bottom as it
 * grows, read every second; the Allow / Deny bar while it waits for a
 * permission; and the composer, with the keys a terminal needs and a photo
 * or a file to attach. A Claude Code pane opens on its chat (Chat.tsx)
 * instead, and shows this behind its Terminal button; the project manager's
 * tab does the same.
 */

export interface ConsoleProps {
  paneId: string;
  name: string;
  /** Under the name: the project, or what this pane is. */
  where: string;
  /** The state known from the PC's state, until the screen says. */
  activity: Activity | null;
  /** Writes the message; the pane's own send or the project manager's. */
  send: (text: string) => Promise<unknown>;
  placeholder: string;
  /** Where an attached file goes; no attaching without one. */
  workspaceId?: string;
  back?: Route;
  /** Where the back button leads when it is not a screen: the chat, from its terminal. */
  onBack?: () => void;
  backLabel?: string;
  /** A tab bar sits below. */
  tabbed?: boolean;
  /** Shown above the screen: why it may be empty. */
  notice?: ReactNode;
}

const KEYS: { key: Key; label: Word; hint: Word; arrow?: boolean }[] = [
  { key: "escape", label: "key.escape", hint: "key.escapeHint" },
  { key: "enter", label: "key.enter", hint: "key.enterHint" },
  { key: "up", label: "key.up", hint: "key.upHint", arrow: true },
  { key: "down", label: "key.down", hint: "key.downHint", arrow: true },
  { key: "tab", label: "key.tab", hint: "key.tabHint" },
  { key: "ctrl+c", label: "key.ctrlC", hint: "key.ctrlCHint" },
];

/** Long enough for the PC to pass the answer on and the screen to show it. */
const ANSWER_SETTLES_MS = 4_000;

export type Status =
  | { kind: "busy"; text: string }
  | { kind: "done"; text: string }
  | { kind: "error"; text: string; retry?: () => void }
  | null;

/**
 * The line over the composer, and the actions that write it: a status that
 * is only news fades; a failure stays, with its retry, until the next action.
 */
export function useStatus() {
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => {
    if (status?.kind !== "done") return;
    const timer = setTimeout(() => setStatus(null), 2_500);
    return () => clearTimeout(timer);
  }, [status]);

  const act = async (run: () => Promise<unknown>, done?: string): Promise<boolean> => {
    try {
      await run();
      setStatus(done ? { kind: "done", text: done } : null);
      return true;
    } catch (error) {
      setStatus({ kind: "error", text: errorText(error), retry: () => void act(run, done) });
      return false;
    }
  };

  return { status, setStatus, act };
}

export function Console(props: ConsoleProps) {
  const { client } = useRemoteContext();
  const [screen, setScreen] = useState<Screen | null>(null);
  const [gone, setGone] = useState(false);
  const { status, setStatus, act } = useStatus();
  /**
   * When the person last answered a permission: the bar steps aside until the
   * pane moves on, or a few seconds, in case it asks again straight away.
   */
  const [answeredAt, setAnsweredAt] = useState<number | null>(null);

  usePolling(async () => {
    try {
      const next = await client.screen(props.paneId);
      setScreen(next);
      setGone(false);
      if (answeredAt !== null && (next.activity !== "waiting" || Date.now() - answeredAt > ANSWER_SETTLES_MS)) {
        setAnsweredAt(null);
      }
    } catch (error) {
      if (error instanceof ApiError && error.kind === "refused") setGone(true);
    }
  }, 1_000);

  const activity = screen ? screen.activity : props.activity;

  const answer = async (allow: boolean) => {
    setAnsweredAt(Date.now());
    const ok = await act(
      () => client.permission(props.paneId, allow),
      allow ? t("permission.allowed") : t("permission.denied"),
    );
    if (!ok) setAnsweredAt(null);
  };

  return (
    <div className="console">
      <PaneHeader {...props} activity={activity} />

      <ScreenText screen={screen} gone={gone} name={props.name} notice={props.notice} />

      {activity === "waiting" && answeredAt === null && !gone && (
        <div className="ask" role="group" aria-label={t("permission.title", { name: props.name })}>
          <p className="ask__title">{t("permission.title", { name: props.name })}</p>
          <p className="ask__hint">{t("permission.hint")}</p>
          <div className="ask__actions">
            <button type="button" className="btn btn--deny" onClick={() => void answer(false)}>
              {t("permission.deny")}
            </button>
            <button type="button" className="btn btn--primary" onClick={() => void answer(true)}>
              {t("permission.allow")}
            </button>
          </div>
        </div>
      )}

      <Composer
        send={props.send}
        placeholder={props.placeholder}
        workspaceId={props.workspaceId}
        tabbed={props.tabbed}
        status={status}
        setStatus={setStatus}
        act={act}
        press={(key) => act(() => client.send({ pane: props.paneId, keys: [key] }))}
        sent={t("composer.sent")}
      />
    </div>
  );
}

/** A pane's name, where it is and its state, under the way back; `aside` at its right. */
export function PaneHeader({
  name,
  where,
  activity,
  back: to,
  onBack,
  backLabel,
  aside,
}: Pick<ConsoleProps, "name" | "where" | "activity" | "back" | "onBack" | "backLabel"> & { aside?: ReactNode }) {
  const { back } = useNavigation();
  const leave = onBack ?? (to ? () => back(to) : null);
  return (
    <header className="console__head">
      {leave && (
        <button type="button" className="back" onClick={leave} aria-label={backLabel ?? t("pane.back")}>
          <Icon name="chevronLeft" />
        </button>
      )}
      <div className={`console__who ${leave ? "" : "console__who--alone"}`}>
        <h1 className="console__name">{name}</h1>
        <p className="console__where">
          <span>{where}</span>
          <StateLabel activity={activity} />
        </p>
      </div>
      {aside}
    </header>
  );
}

/** The screen's text, kept at the bottom unless the person scrolled up to read. */
function ScreenText({ screen, gone, name, notice }: { screen: Screen | null; gone: boolean; name: string; notice?: ReactNode }) {
  const scroller = useRef<HTMLDivElement>(null);
  const [following, setFollowing] = useState(true);
  const lines = useMemo(() => screenLines(screen?.text ?? ""), [screen?.text]);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (element && following) element.scrollTop = element.scrollHeight;
  }, [lines, following]);

  const onScroll = () => {
    const element = scroller.current;
    if (!element) return;
    setFollowing(element.scrollHeight - element.scrollTop - element.clientHeight < 40);
  };

  const hasText = lines.some((line) => line.kind === "text" && line.text);
  return (
    <div className="screen">
      <div className="screen__scroll" ref={scroller} onScroll={onScroll} aria-label={t("pane.screen", { name })}>
        {notice}
        {gone ? (
          <p className="screen__note">{t("pane.gone")}</p>
        ) : !screen ? (
          <p className="screen__note">{t("pane.loading")}</p>
        ) : !hasText ? (
          <p className="screen__note">{t("pane.empty")}</p>
        ) : (
          <pre className="screen__text">
            {lines.map((line, index) =>
              line.kind === "rule" ? (
                <span key={index} className="screen__rule" />
              ) : (
                <span key={index} className="screen__line">
                  {line.text}
                </span>
              ),
            )}
          </pre>
        )}
      </div>
      {!following && (
        <button type="button" className="to-end" onClick={() => setFollowing(true)}>
          <Icon name="arrowDown" />
          {t("pane.toEnd")}
        </button>
      )}
    </div>
  );
}

interface ComposerProps {
  send: (text: string) => Promise<unknown>;
  placeholder: string;
  workspaceId?: string;
  tabbed?: boolean;
  status: Status;
  setStatus: (status: Status) => void;
  act: (run: () => Promise<unknown>, done?: string) => Promise<boolean>;
  /** The terminal's keys; the chat keeps them behind its Terminal button. */
  press?: (key: Key) => Promise<boolean>;
  /** Said once a message is sent; the chat shows the message itself instead. */
  sent?: string;
  /** Above the status: the chat's "Claude is working". */
  children?: ReactNode;
}

export function Composer({
  send,
  placeholder,
  workspaceId,
  tabbed,
  status,
  setStatus,
  act,
  press,
  sent,
  children,
}: ComposerProps) {
  const { client } = useRemoteContext();
  const [text, setText] = useState("");
  const [uploading, setUploading] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const picker = useRef<HTMLInputElement>(null);

  // The field grows with what is typed, up to a few lines.
  useLayoutEffect(() => {
    const element = field.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${element.scrollHeight}px`;
  }, [text]);

  const submit = async () => {
    const message = text.trim();
    if (!message) return;
    if (await act(() => send(message), sent)) setText("");
  };

  const attach = async (file: File) => {
    if (!workspaceId) return;
    setUploading(true);
    setStatus({ kind: "busy", text: t("composer.uploading", { name: file.name }) });
    try {
      const { path } = await client.upload(workspaceId, file, file.name);
      // The path goes where the agent will read it: into the message.
      setText((current) => (current && !/\s$/.test(current) ? `${current} ${path} ` : `${current}${path} `));
      setStatus({ kind: "done", text: t("composer.uploaded", { name: file.name }) });
      field.current?.focus();
    } catch (error) {
      setStatus({ kind: "error", text: errorText(error), retry: () => void attach(file) });
    } finally {
      setUploading(false);
    }
  };

  return (
    <div className={`composer ${tabbed ? "composer--tabbed" : ""}`}>
      {children}
      {status && (
        <p className={`status status--${status.kind}`} role={status.kind === "error" ? "alert" : "status"}>
          {status.kind === "done" && <Icon name="check" size={16} />}
          <span>{status.text}</span>
          {status.kind === "error" && status.retry && (
            <button type="button" className="btn" onClick={status.retry}>
              {t("error.retry")}
            </button>
          )}
        </p>
      )}
      {press && (
        <div className="keys" role="group" aria-label={t("composer.keys")}>
          {KEYS.map(({ key, label, hint, arrow }) => (
            <button
              key={key}
              type="button"
              className={`key ${arrow ? "key--arrow" : ""}`}
              aria-label={t(hint)}
              onClick={() => void press(key)}
            >
              {t(label)}
            </button>
          ))}
        </div>
      )}
      <form
        className={`compose ${press ? "" : "compose--alone"}`}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        {workspaceId && (
          <>
            <button
              type="button"
              className="compose__icon"
              aria-label={t("composer.attach")}
              disabled={uploading}
              onClick={() => picker.current?.click()}
            >
              <Icon name="paperclip" />
            </button>
            <input
              ref={picker}
              type="file"
              hidden
              onChange={(event) => {
                const file = event.target.files?.[0];
                event.target.value = "";
                if (file) void attach(file);
              }}
            />
          </>
        )}
        <textarea
          ref={field}
          className="compose__field"
          rows={1}
          value={text}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            // A keyboard's Enter sends, Shift+Enter starts a line; a phone's
            // return key starts a line, and the button sends.
            if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing && matchMedia("(pointer: fine)").matches) {
              event.preventDefault();
              void submit();
            }
          }}
        />
        <button type="submit" className="compose__icon compose__send" aria-label={t("composer.send")} disabled={!text.trim()}>
          <Icon name="arrowUp" />
        </button>
      </form>
    </div>
  );
}
