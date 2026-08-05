import { useEffect, useState } from "react";
// The very file Discord fetches from the repository, bundled in: the preview
// stays right without a round trip, and there is only ever one logo to update.
import logo from "../../../assets/logo.png";
import type { Translator } from "../../i18n";
import type { DiscordPresence } from "../../lib/discord";
import { discordPresenceStatus } from "../../lib/ipc";

/**
 * Shows the presence as Discord will render it, and whether the link is live.
 *
 * Worth the pixels: the card is published to other people, and the settings
 * that shape it (name the workspace? name the agents?) are only meaningful once
 * you can see what they leave on screen. The status line answers the other
 * question a toggle cannot — nothing appeared, is that this app or is Discord
 * simply closed?
 */

/** How often the connection state is re-read while the page is open. */
const POLL_MS = 2000;

const pad = (n: number) => String(n).padStart(2, "0");

function elapsedSince(startedAt: number): string {
  const total = Math.max(0, Math.floor((Date.now() - startedAt) / 1000));
  const hours = Math.floor(total / 3600);
  return `${pad(hours)}:${pad(Math.floor(total / 60) % 60)}:${pad(total % 60)}`;
}

function StatusLine({ t, enabled }: { t: Translator; enabled: boolean }) {
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) {
      setConnected(false);
      return;
    }
    let cancelled = false;
    const read = () =>
      discordPresenceStatus()
        .then((status) => {
          if (cancelled) return;
          setConnected(status.connected);
          setError(status.error);
        })
        .catch(() => undefined);
    read();
    const timer = window.setInterval(read, POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [enabled]);

  const tone = !enabled ? "off" : connected ? "on" : "waiting";
  const label = !enabled
    ? t("discord.statusOff")
    : connected
      ? t("discord.statusConnected")
      : t("discord.statusWaiting");

  return (
    <p className={`dc-status dc-status--${tone}`}>
      <i className="dc-status__dot" aria-hidden="true" />
      <span className="dc-status__label">{label}</span>
      {/* The IPC error is jargon on its own line; it earns a tooltip, not a row. */}
      {tone === "waiting" && (
        <span className="dc-status__hint" title={error ?? undefined}>
          {t("discord.statusWaitingHint")}
        </span>
      )}
    </p>
  );
}

export function DiscordCard({
  t,
  presence,
  enabled,
}: {
  t: Translator;
  /** Always built, even while off, so the toggles can be judged before use. */
  presence: DiscordPresence;
  enabled: boolean;
}) {
  const [, tick] = useState(0);

  // The elapsed timer is the one part of the card that moves on its own.
  useEffect(() => {
    const timer = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <div className="dc">
      <div className={`dc-card ${enabled ? "" : "is-off"}`}>
        <div className="dc-card__art" title={presence.largeUrl ?? presence.largeText}>
          <img className="dc-card__icon" src={logo} alt="" />
          {presence.smallText && (
            <span className="dc-card__badge" title={presence.smallText}>
              {presence.smallText.slice(0, 1)}
            </span>
          )}
        </div>

        <div className="dc-card__lines">
          <span className="dc-card__head">{t("discord.cardHead")}</span>
          <strong className="dc-card__name">Hangar.AI</strong>
          <span className="dc-card__details">{presence.details}</span>
          <span className="dc-card__state">{presence.state}</span>
          {presence.startedAt !== null && (
            <span className="dc-card__time">
              {t("discord.elapsed", { time: elapsedSince(presence.startedAt) })}
            </span>
          )}
          {presence.buttonLabel && (
            <span className="dc-card__button" title={presence.buttonUrl ?? undefined}>
              {presence.buttonLabel}
            </span>
          )}
        </div>
      </div>

      <StatusLine t={t} enabled={enabled} />
    </div>
  );
}
