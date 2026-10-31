/**
 * The strings of Settings › Kits and of the dialog that enables a kit on a
 * workspace. i18n.ts merges both dictionaries into its own, so a key reads
 * through `t()` like any other; keys start with `kitsManage.`.
 *
 * `en` is the reference. `fr` is typed against it, so the build fails on a
 * French key that is missing or misspelled — here, without i18n.ts having
 * to change. French follows the glossary and the typography of i18n.ts:
 * plain spaces before ? ! ; : » and after «, the typesetter does the rest.
 */

export const en = {
  "kitsManage.title": "Kits",
} as const;

export const fr: Record<keyof typeof en, string> = {
  "kitsManage.title": "Kits",
};
