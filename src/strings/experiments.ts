/**
 * The strings of the Experimental page of the settings: the page itself, not
 * the features on it. Each feature brings its own title and hint, in its own
 * strings module, and names them in `lib/experiments`. i18n.ts merges both
 * dictionaries into its own, so a key reads through `t()` like any other; keys
 * start with `experiments.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled. French follows the glossary and
 * the typography of i18n.ts: plain spaces before ? ! ; : » and after «, the
 * typesetter does the rest.
 */

export const en = {
  "settings.catExperimental": "Experimental",
  "experiments.title": "Experimental features",
  "experiments.warning":
    "Unfinished work. These features are off until you turn them on, and they can change, break or disappear in any update.",
  "experiments.stage.experimental": "Experimental",
  "experiments.stage.beta": "Beta",
  "experiments.onlyOn": "Only on {platforms}",
};

export const fr: Record<keyof typeof en, string> = {
  "settings.catExperimental": "Expérimental",
  "experiments.title": "Fonctionnalités expérimentales",
  "experiments.warning":
    "Du travail en cours. Ces fonctionnalités restent désactivées tant que vous ne les activez pas, et elles peuvent changer, casser ou disparaître à n'importe quelle mise à jour.",
  "experiments.stage.experimental": "Expérimental",
  "experiments.stage.beta": "Bêta",
  "experiments.onlyOn": "Uniquement sur {platforms}",
};
