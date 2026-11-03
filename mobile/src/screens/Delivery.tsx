import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ApiError, type Delivery, type DeliveryPatch } from "../api";
import { BackBar, EmptyState, Link, useNavigation } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { Markdown } from "../components/Markdown";
import {
  checkVerdict,
  fileRows,
  landBlocked,
  landUntilDone,
  outcomeLabel,
  patchLines,
  remember,
  rememberedFile,
  reviewVerdict,
  reworkText,
  stageText,
  totals,
  type LandStatus,
  type Tone,
} from "../delivery";
import { splitPath } from "../../lib/diff";
import { errorText } from "../errors";
import { ago } from "../format";
import { usePolling } from "../hooks";
import { i18n, locale, t, tn } from "../i18n";
import { useRemoteContext } from "../remote";
import type { Route } from "../route";

/** What waits is read again this often while it is on screen: another delivery may take its place. */
const REFRESH_MS = 5_000;

const TONE_ICON = { ok: "check", failed: "failed", none: "clock" } as const;

/**
 * A delivery ready to merge: what its worker changed and what the check and
 * the review said, then « Fusionner », or « Renvoyer » with what to fix. The
 * landing shows where it is on the PC until it lands or says why not.
 */
export function DeliveryScreen({ task }: { task: string }) {
  const { client, state } = useRemoteContext();
  const { back } = useNavigation();
  const [delivery, setDelivery] = useState<Delivery | null>(null);
  /** Refused by the PC: it no longer waits, and why. */
  const [gone, setGone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [land, setLand] = useState<LandStatus>({ kind: "idle" });
  const [reworking, setReworking] = useState(false);
  const [sent, setSent] = useState(false);
  const mounted = useRef(true);
  useEffect(
    () => () => {
      mounted.current = false;
    },
    [],
  );

  const refresh = useCallback(async () => {
    try {
      const next = await client.delivery(task);
      if (!mounted.current) return;
      remember(next);
      setDelivery(next);
      setGone(null);
      setError(null);
    } catch (failure) {
      if (!mounted.current) return;
      if (failure instanceof ApiError && failure.kind === "refused") setGone(failure.detail ?? failure.code);
      else setError(errorText(failure));
    }
  }, [client, task]);

  // Read again until something is done about it: a landing, or sending it back.
  usePolling(refresh, REFRESH_MS, land.kind === "idle" && !sent);

  const project = delivery ? state?.projects.find((candidate) => candidate.id === delivery.workspaceId) : undefined;
  const home: Route = project ? { name: "project", id: project.id } : { name: "forYou" };
  const backBar = <BackBar to={home} label={project?.name ?? t("forYou.title")} />;

  const startLanding = async () => {
    if (!delivery) return;
    setLand({ kind: "landing", stage: delivery.stage });
    const outcome = await landUntilDone(client, delivery.taskId, delivery.merge, {
      onStage: (stage) => mounted.current && setLand({ kind: "landing", stage }),
      stopped: () => !mounted.current,
    });
    if (mounted.current) setLand(outcome);
  };

  const dismiss = () => {
    setLand({ kind: "idle" });
    void refresh();
  };

  if (!delivery) {
    // Landed or sent back and gone from the board since: what was done stays said.
    return (
      <div className="page">
        {backBar}
        {gone !== null ? (
          <EmptyState title={t("delivery.gone")} text={gone} mark={<Icon name="flag" size={40} />} />
        ) : error ? (
          <EmptyState title={error}>
            <button type="button" className="btn" onClick={() => void refresh()}>
              {t("error.retry")}
            </button>
          </EmptyState>
        ) : (
          <p className="calm" role="status">
            {t("delivery.loading")}
          </p>
        )}
      </div>
    );
  }

  const check = checkVerdict(delivery.check, i18n);
  const review = reviewVerdict(delivery.review, i18n);
  const rows = fileRows(delivery.files, i18n);
  const sum = totals(delivery.files);
  const blocked = landBlocked(delivery);

  return (
    <div className="with-actions">
      <div className="page delivery" aria-label={t("delivery.label", { title: delivery.title, worker: delivery.worker })}>
        {backBar}
        <p className="delivery__where">
          {delivery.workspace} · {delivery.worker}
        </p>
        <h1 className="delivery__title">{delivery.title}</h1>
        <p className="delivery__branch">
          <code>{delivery.branch}</code>
          <Icon name="arrowRight" size={16} />
          <code>{delivery.target}</code>
        </p>
        {delivery.passedAt !== null && (
          <p className="delivery__when">{t("delivery.passed", { time: ago(delivery.passedAt, Date.now(), locale) })}</p>
        )}

        {gone !== null && land.kind === "idle" && !sent && <Notice tone="failed" text={`${t("delivery.gone")}. ${gone}`} />}
        {blocked === "running" && land.kind === "idle" && (
          <Notice tone="none" text={t("delivery.running", { stage: stageText(delivery.stage, i18n) })} busy />
        )}
        {blocked === "moved" && <Notice tone="failed" text={t("delivery.moved", { worker: delivery.worker })} />}
        {delivery.inTheWay && (
          <Notice
            tone="failed"
            text={t("delivery.inTheWay", { target: delivery.target })}
            files={delivery.inTheWay.length ? delivery.inTheWay : undefined}
          />
        )}

        <ul className="verdicts">
          <li className={`verdict verdict--${check.tone}`}>
            <Icon name={TONE_ICON[check.tone]} />
            <div className="verdict__body">
              <p className="verdict__head">
                <span className="verdict__what">{t("delivery.check")}</span>
                <span className="verdict__title">{check.title}</span>
              </p>
              {check.detail && <code className="verdict__command">{check.detail}</code>}
            </div>
          </li>
          <li className={`verdict verdict--${review.tone}`}>
            <Icon name={TONE_ICON[review.tone]} />
            <div className="verdict__body">
              <p className="verdict__head">
                <span className="verdict__what">{t("delivery.review")}</span>
                <span className="verdict__title">{review.title}</span>
              </p>
              {review.notes && <p className="verdict__notes">{review.notes}</p>}
              {delivery.review && delivery.review.blocking.length > 0 && (
                <Blocking title={t("delivery.review.blocking")} items={delivery.review.blocking} />
              )}
            </div>
          </li>
        </ul>

        {delivery.earlier.length > 0 && (
          <details className="earlier">
            <summary>
              <Icon name="chevronRight" size={16} className="earlier__chevron" />
              {tn("delivery.earlier", delivery.earlier.length)}
            </summary>
            <ol className="earlier__list">
              {delivery.earlier.map((attempt, index) => (
                <li key={index}>
                  <p className="earlier__head">
                    <span className="earlier__what">{outcomeLabel(attempt.outcome, i18n)}</span>
                    <span className="earlier__when">{ago(attempt.at, Date.now(), locale)}</span>
                  </p>
                  <p className="earlier__text">{attempt.text}</p>
                  {attempt.blocking && <Blocking items={attempt.blocking} />}
                </li>
              ))}
            </ol>
          </details>
        )}

        {delivery.summary && (
          <section className="section">
            <div className="section__head">
              <h2 className="section__title">{t("delivery.summary", { worker: delivery.worker })}</h2>
            </div>
            <Markdown text={delivery.summary} className="panel delivery__summary" />
          </section>
        )}

        <section className="section">
          <div className="section__head">
            <h2 className="section__title">{t("delivery.files")}</h2>
            <span className="section__count" aria-label={t("delivery.lines", { add: sum.additions, del: sum.deletions })}>
              {delivery.files.length} · <span className="lines--add">+{sum.additions}</span>{" "}
              <span className="lines--del">−{sum.deletions}</span>
            </span>
          </div>
          {rows.length === 0 ? (
            <p className="section__hint">{t("delivery.noFiles")}</p>
          ) : (
            <ul className="list changes">
              {rows.map((row) => (
                <li key={row.file.path}>
                  <Link to={{ name: "diff", task: delivery.taskId, file: row.file.path }} className="row change">
                    <span className={`change__mark change__mark--${row.file.status}`} title={row.label}>
                      <span aria-hidden="true">{row.mark}</span>
                      <span className="sr-only">{row.label}</span>
                    </span>
                    <span className="row__main">
                      <span className="change__name">{row.name}</span>
                      <span className="change__dir">
                        {row.file.oldPath ? t("delivery.renamedFrom", { path: row.file.oldPath }) : row.dir || " "}
                      </span>
                    </span>
                    {row.counts ? (
                      <span className="change__counts">
                        <span className="lines--add">+{row.file.additions}</span>
                        <span className="lines--del">−{row.file.deletions}</span>
                      </span>
                    ) : (
                      <span className="tag">{t("delivery.binary")}</span>
                    )}
                    <Icon name="chevronRight" className="chevron" />
                  </Link>
                </li>
              ))}
            </ul>
          )}
          {delivery.moreFiles && <p className="section__hint delivery__more">{t("delivery.filesMore")}</p>}
        </section>
      </div>

      <div className="actionbar">
        {sent ? (
          <Outcome tone="ok" title={t("delivery.sent", { worker: delivery.worker })}>
            <button type="button" className="btn" onClick={() => back(home)}>
              {t("delivery.back")}
            </button>
          </Outcome>
        ) : land.kind === "landing" ? (
          <p className="landing" role="status">
            <span className="spinner" aria-hidden="true" />
            <span className="landing__text">
              <span className="landing__title">{t("delivery.landing")}</span>
              {land.stage && <span className="landing__stage">{t("delivery.landingStage", { stage: stageText(land.stage, i18n) })}</span>}
            </span>
          </p>
        ) : land.kind === "landed" ? (
          <Outcome tone="ok" title={t("delivery.landed", { target: land.target })} text={t("delivery.landedHint")}>
            <button type="button" className="btn btn--primary" onClick={() => back(home)}>
              {t("delivery.back")}
            </button>
          </Outcome>
        ) : land.kind === "refused" ? (
          <Outcome tone="failed" title={t("delivery.notLanded")} text={land.reason}>
            <button type="button" className="btn" onClick={dismiss}>
              {t("delivery.ok")}
            </button>
          </Outcome>
        ) : land.kind === "lost" ? (
          <Outcome tone="failed" title={t("delivery.lost")}>
            <button type="button" className="btn btn--primary" onClick={() => void startLanding()}>
              {t("delivery.lookAgain")}
            </button>
          </Outcome>
        ) : reworking ? (
          <Rework
            delivery={delivery}
            onCancel={() => setReworking(false)}
            onSent={() => {
              setReworking(false);
              setSent(true);
            }}
          />
        ) : (
          <div className="actionbar__buttons">
            <button type="button" className="btn" onClick={() => setReworking(true)} disabled={gone !== null}>
              {t("delivery.rework")}
            </button>
            <button
              type="button"
              className="btn btn--primary actionbar__main"
              onClick={() => void startLanding()}
              disabled={blocked !== null || gone !== null}
            >
              {t("delivery.land")}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

function Notice({ tone, text, files, busy }: { tone: Tone; text: string; files?: string[]; busy?: boolean }) {
  return (
    <div className={`delivery__notice delivery__notice--${tone}`} role={tone === "failed" ? "alert" : "status"}>
      {busy ? <span className="spinner" aria-hidden="true" /> : <Icon name="alert" size={18} />}
      <div>
        <p>{text}</p>
        {files && (
          <ul className="delivery__files">
            {files.map((file) => (
              <li key={file}>
                <code>{file}</code>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

function Blocking({ title, items }: { title?: string; items: { file: string; line?: number; problem: string }[] }) {
  return (
    <div className="blocking">
      {title && <p className="blocking__title">{title}</p>}
      <ul>
        {items.map((item, index) => (
          <li key={index}>
            <code>
              {item.file}
              {item.line ? `:${item.line}` : ""}
            </code>{" "}
            {item.problem}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Outcome({ tone, title, text, children }: { tone: Tone; title: string; text?: string; children: ReactNode }) {
  return (
    <div className={`outcome outcome--${tone}`} role={tone === "failed" ? "alert" : "status"}>
      <Icon name={TONE_ICON[tone]} size={22} />
      <div className="outcome__body">
        <p className="outcome__title">{title}</p>
        {text && <p className="outcome__text">{text}</p>}
      </div>
      {children}
    </div>
  );
}

/** « Renvoyer »: what the worker should fix, sent as the person's own words. */
function Rework({ delivery, onCancel, onSent }: { delivery: Delivery; onCancel: () => void; onSent: () => void }) {
  const { client } = useRemoteContext();
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  useEffect(() => field.current?.focus(), []);
  const ready = reworkText(text) !== "" && !sending;

  const send = async () => {
    if (!ready) return;
    setSending(true);
    setError(null);
    try {
      await client.rework(delivery.taskId, reworkText(text), delivery.merge);
      onSent();
    } catch (failure) {
      setError(failure instanceof ApiError && failure.kind === "refused" ? (failure.detail ?? errorText(failure)) : errorText(failure));
      setSending(false);
    }
  };

  return (
    <form
      className="rework"
      onSubmit={(event) => {
        event.preventDefault();
        void send();
      }}
    >
      <label className="rework__label" htmlFor="rework-text">
        {t("delivery.reworkLabel")}
      </label>
      {error && (
        <p className="status status--error" role="alert">
          {error}
        </p>
      )}
      <textarea
        id="rework-text"
        ref={field}
        className="compose__field rework__field"
        rows={3}
        value={text}
        placeholder={t("delivery.reworkPlaceholder", { worker: delivery.worker })}
        onChange={(event) => setText(event.target.value)}
        enterKeyHint="send"
      />
      <div className="actionbar__buttons">
        <button type="button" className="btn" onClick={onCancel} disabled={sending}>
          {t("delivery.cancel")}
        </button>
        <button type="submit" className="btn btn--primary actionbar__main" disabled={!ready}>
          {sending ? <span className="spinner" aria-hidden="true" /> : null}
          {t("delivery.send")}
        </button>
      </div>
    </form>
  );
}

/** One file of a delivery: its unified diff, its lines coloured, scrolled sideways inside its block. */
export function DiffScreen({ task, file }: { task: string; file: string }) {
  const { client } = useRemoteContext();
  const [patch, setPatch] = useState<DeliveryPatch | null>(null);
  const [error, setError] = useState<string | null>(null);
  const known = rememberedFile(task, file);
  const { dir, name } = splitPath(file);

  const load = useCallback(() => {
    let current = true;
    setError(null);
    client.deliveryFile(task, file).then(
      (answer) => current && setPatch(answer),
      (failure) =>
        current &&
        setError(failure instanceof ApiError && failure.kind === "refused" ? (failure.detail ?? errorText(failure)) : errorText(failure)),
    );
    return () => {
      current = false;
    };
  }, [client, task, file]);
  useEffect(load, [load]);

  const lines = useMemo(() => (patch && !patch.binary ? patchLines(patch.patch) : []), [patch]);

  return (
    <div className="page diff">
      <BackBar to={{ name: "delivery", task }} label={t("delivery.title")} />
      <h1 className="diff__title">
        <span className="diff__name">{name}</span>
        {dir && <span className="diff__dir">{dir}</span>}
      </h1>
      {known && !known.binary && (
        <p className="diff__counts" aria-label={t("delivery.lines", { add: known.additions, del: known.deletions })}>
          <span>{t(`delivery.status.${known.status}`)}</span>
          <span className="lines--add">+{known.additions}</span>
          <span className="lines--del">−{known.deletions}</span>
        </p>
      )}
      {error ? (
        <EmptyState title={error}>
          <button type="button" className="btn" onClick={load}>
            {t("error.retry")}
          </button>
        </EmptyState>
      ) : !patch ? (
        <p className="calm" role="status">
          {t("diff.loading")}
        </p>
      ) : patch.binary ? (
        <p className="calm">{t("diff.binary")}</p>
      ) : lines.length === 0 ? (
        <p className="calm">{t("diff.empty")}</p>
      ) : (
        <>
          <pre className="patch" tabIndex={0} aria-label={t("diff.label", { file: name })}>
            {lines.map((line, index) => (
              <span key={index} className={`patch__line patch__line--${line.kind}`}>
                {line.kind === "hunk" || line.kind === "note"
                  ? line.text
                  : `${line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "} ${line.text}`}
              </span>
            ))}
          </pre>
          {patch.truncated && (
            <p className="diff__cut">
              <Icon name="alert" size={16} />
              {t("diff.cut")}
            </p>
          )}
        </>
      )}
    </div>
  );
}
