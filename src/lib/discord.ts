/**
 * Discord Rich Presence — what the card says, and when it is republished.
 *
 * The Rust side owns the connection and the rate limiting; everything here is
 * about the shape of the presence. It is rebuilt from the state on every
 * render and only crosses the IPC boundary when it actually differs, so the
 * worker can compare payloads instead of guessing what changed.
 */

import { useEffect } from "react";
import { setDiscordPresence } from "./ipc";
import type { Translator } from "../i18n";
import { AGENTS, type Pane, type Settings, type Workspace } from "../types";

/**
 * The Discord application this app ships with. Its uploaded art is what the
 * asset keys below resolve to; a fork with its own art overrides it from the
 * settings rather than by rebuilding.
 */
export const DEFAULT_APP_ID = "1534592584045957282";

export const REPO_URL = "https://github.com/simnJS/hangar-ai";

/**
 * The logo, served straight from the repository — the same file the README
 * shows, so the two never drift apart.
 *
 * Discord takes a URL as readily as an uploaded asset key: it proxies the image
 * and caches the result, which saves everyone a trip to the developer portal.
 * The cost is that the URL has to resolve on `main`. The badge below has no
 * such option — agent art has to be uploaded under these keys — but Discord
 * drops the keys it cannot resolve without complaining, so a card missing its
 * art is still a card.
 */
const LARGE_IMAGE = "https://raw.githubusercontent.com/simnJS/hangar-ai/main/assets/logo.png";
const agentImage = (agent: string) => `agent-${agent}`;

/**
 * Discord shows the elapsed time counting up from here. The app launch is the
 * only start that stays put: pinning it to the open workspace would reset the
 * timer on every switch, which reads as "just started" all day long.
 */
const STARTED_AT = Date.now();

/** Mirrors the `Presence` struct on the Rust side. */
export interface DiscordPresence {
  appId: string;
  details: string;
  state: string;
  largeImage: string;
  largeText: string;
  largeUrl: string | null;
  smallImage: string | null;
  smallText: string | null;
  startedAt: number | null;
  buttonLabel: string | null;
  buttonUrl: string | null;
}

export interface DiscordStatus {
  enabled: boolean;
  connected: boolean;
  error: string | null;
}

const agentLabel = (id: string) => AGENTS.find((a) => a.id === id)?.label ?? id;

export interface PresenceInput {
  settings: Settings;
  workspace: Workspace | null;
  /** The pane in focus, whose agent gets the small badge. */
  focusedPaneId: string | null;
  t: Translator;
}

/**
 * Builds the card, or `null` when the feature is off — which is also what tells
 * the worker to take the presence down.
 */
export function buildPresence({
  settings,
  workspace,
  focusedPaneId,
  t,
}: PresenceInput): DiscordPresence | null {
  if (!settings.discordPresence) return null;

  const panes: Pane[] = workspace?.panes ?? [];
  // A shell is a terminal, not an agent: counting it would claim five agents
  // are running on a workspace where nothing was launched.
  const working = panes.filter((pane) => pane.agent !== "shell");
  const kinds = [...new Set(working.map((pane) => pane.agent))];

  const details = workspace
    ? settings.discordShowWorkspace
      ? workspace.name
      : t("discord.working")
    : t("discord.noWorkspace");

  const state = !workspace
    ? t("discord.idle")
    : [
        t("discord.panes", { n: panes.length }),
        kinds.length === 0
          ? t("discord.shellOnly")
          : settings.discordShowAgents
            ? kinds.map(agentLabel).join(", ")
            : t("discord.agents", { n: kinds.length }),
      ].join(" · ");

  // The badge follows the pane you are looking at, falling back to whichever
  // agent is running — a shell-only workspace gets no badge rather than a
  // meaningless one.
  const focused = panes.find((pane) => pane.id === focusedPaneId);
  const badge =
    settings.discordShowAgents && kinds.length > 0
      ? (focused && focused.agent !== "shell" ? focused.agent : kinds[0])
      : null;

  return {
    appId: settings.discordAppId.trim() || DEFAULT_APP_ID,
    details,
    state,
    largeImage: LARGE_IMAGE,
    largeText: t("discord.tagline"),
    // The repository is reachable two ways: the button, and the art itself.
    largeUrl: REPO_URL,
    smallImage: badge ? agentImage(badge) : null,
    smallText: badge ? agentLabel(badge) : null,
    startedAt: STARTED_AT,
    buttonLabel: t("discord.button"),
    buttonUrl: REPO_URL,
  };
}

/**
 * Publishes `presence` whenever it changes, and takes it down on unmount.
 *
 * The effect keys off the serialized payload rather than the object: it is
 * rebuilt on every render, so an identity check would republish constantly and
 * an update every few seconds is all Discord accepts.
 */
export function useDiscordPresence(presence: DiscordPresence | null) {
  const payload = presence ? JSON.stringify(presence) : "";

  useEffect(() => {
    setDiscordPresence(payload ? (JSON.parse(payload) as DiscordPresence) : null).catch(
      () => undefined,
    );
  }, [payload]);

  useEffect(
    () => () => {
      setDiscordPresence(null).catch(() => undefined);
    },
    [],
  );
}
