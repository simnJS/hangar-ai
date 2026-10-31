/**
 * The strings of a kit's tab: the frame around its page, and what Hangar
 * says about the calls it makes. i18n.ts merges both dictionaries into its
 * own, so a key reads through `t()` like any other; keys start with
 * `kitsTab.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled — here, without i18n.ts having
 * to change. French follows the glossary and the typography of i18n.ts:
 * plain spaces before ? ! ; : » and after «, the typesetter does the rest.
 */

export const en = {
  "kitsTab.loadFailed": "This kit's tab could not be loaded.",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "kitsTab.loadFailed": "L'onglet de ce kit n'a pas pu être chargé.",
};
