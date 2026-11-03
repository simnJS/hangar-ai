import type { TranslateKey } from "../i18n";
import type { Settings } from "../types";

/**
 * Experimental features: work that is not finished, ships off, and that anyone
 * can turn on from Settings → Experimental.
 *
 * To add one, add an entry to `EXPERIMENTS` and its title and hint to a strings
 * module — nothing else draws or stores it. Code that wants to know whether it
 * is on asks `isExperimentEnabled(settings, "your-id")`. The page appears on its
 * own once the list is not empty.
 *
 * What is on is `settings.experiments`, a list of ids, so nothing is on for
 * someone who never opened the page, and an id that has since left the list (a
 * feature that became standard, or was dropped) is ignored rather than an error.
 * A feature that graduates loses its entry here and its check in the code.
 */

export type ExperimentStage = "experimental" | "beta";

export type Platform = "linux" | "windows" | "macos";

export interface Experiment {
  /** Stable, lowercase with dashes: it is what a saved settings file holds. */
  id: string;
  stage: ExperimentStage;
  title: TranslateKey;
  hint: TranslateKey;
  /** Where it can run. Left out, it is offered everywhere. */
  platforms?: readonly Platform[];
}

/** Newest last. Empty until the first experiment lands. */
export const EXPERIMENTS: readonly Experiment[] = [];

/** The platform the webview runs on, as far as its user agent says. */
export function currentPlatform(
  userAgent: string = typeof navigator === "undefined" ? "" : navigator.userAgent,
): Platform | null {
  if (/android|iphone|ipad/i.test(userAgent)) return null;
  if (/windows/i.test(userAgent)) return "windows";
  if (/mac/i.test(userAgent)) return "macos";
  if (/linux|x11/i.test(userAgent)) return "linux";
  return null;
}

/** The experiments this machine can offer: those with no platform list, or one that names it. */
export function availableExperiments(
  registry: readonly Experiment[] = EXPERIMENTS,
  platform: Platform | null = currentPlatform(),
): Experiment[] {
  return registry.filter(
    (experiment) =>
      !experiment.platforms || (platform !== null && experiment.platforms.includes(platform)),
  );
}

/**
 * True when the person turned this experiment on and this machine can run it.
 * Anything unknown is off: a list left by an older version, a hand-edited file,
 * an id nobody registers.
 */
export function isExperimentEnabled(
  settings: Pick<Settings, "experiments">,
  id: string,
  registry: readonly Experiment[] = EXPERIMENTS,
  platform: Platform | null = currentPlatform(),
): boolean {
  if (!Array.isArray(settings.experiments) || !settings.experiments.includes(id)) return false;
  return availableExperiments(registry, platform).some((experiment) => experiment.id === id);
}

/** The list with `id` on or off, without a duplicate and in the order it was turned on. */
export function withExperiment(current: readonly string[], id: string, on: boolean): string[] {
  const rest = (Array.isArray(current) ? current : []).filter((entry) => entry !== id);
  return on ? [...rest, id] : rest;
}
