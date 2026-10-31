"use strict";
// The only text this tab ever sends to an agent. Every sentence is fixed here; what
// varies is the file's path, key names and counts. It is handed names, never a
// value: `values` is only there so that a name that is also a value stays unsaid.
(function (root, factory) {
  const api = factory();
  if (typeof module === "object" && module.exports) module.exports = api;
  else root.EditorAnswer = api;
})(typeof globalThis !== "undefined" ? globalThis : this, function () {
  const NAME = /^[A-Za-z_][A-Za-z0-9_.-]{0,63}$/;
  const NAMED_MAX = 40;
  const SHORTEST_INSIDE = 4; // shorter values match names by chance: `1` in KEY1
  const STAY = "The values stay in the file: do not read them back, use them through the program that loads it.";

  /**
   * A value pasted where a key goes (`sk_live_abc=`, or a value that repeats as
   * another line's key) is still a key to any parser. So a name is said only if it
   * looks like one, is one this file or the agent already knew, and holds no value.
   */
  function mayName(name, known, values) {
    if (typeof name !== "string" || !NAME.test(name) || !known.has(name)) return false;
    return !values.some((raw) => {
      const value = String(raw).trim();
      return value !== "" && (value === name || (value.length >= SHORTEST_INSIDE && name.includes(value)));
    });
  }

  /** The names that may be said, and how many may not. */
  function sayable(names, known, values) {
    const named = [];
    let others = 0;
    for (const name of names) {
      if (named.length < NAMED_MAX && mayName(name, known, values)) named.push(name);
      else others++;
    }
    return { named, others };
  }

  function sentence(label, names) {
    return names.length ? ` ${label}: ${names.join(", ")}.` : "";
  }

  const others = (count, verb) => (count ? ` ${count} other ${count === 1 ? "key" : "keys"} ${verb}.` : "");

  /** The person saved a dotenv file. */
  function dotenvSaved({ path, set, emptied, stillEmpty, known, values }) {
    const setSaid = sayable(set, known, values);
    const emptiedSaid = sayable(emptied, known, values);
    const emptySaid = sayable(stillEmpty, known, values);
    return (
      `The person saved \`${path}\`.` +
      sentence("Set", setSaid.named) +
      sentence("Emptied", emptiedSaid.named) +
      sentence("Still empty", emptySaid.named) +
      others(setSaid.others + emptiedSaid.others + emptySaid.others, "changed or empty") +
      ` ${STAY}`
    );
  }

  /** The person looked at a dotenv file and had nothing to change. */
  function dotenvChecked({ path, alreadySet, stillEmpty, known, values }) {
    const setSaid = sayable(alreadySet, known, values);
    const emptySaid = sayable(stillEmpty, known, values);
    return (
      `The person checked \`${path}\` and changed nothing.` +
      sentence("Already set", setSaid.named) +
      sentence("Still empty", emptySaid.named) +
      others(setSaid.others + emptySaid.others, "set or empty") +
      ` ${STAY}`
    );
  }

  const textSaved = ({ path }) => `The person saved \`${path}\`: the file changed.`;
  const textChecked = ({ path }) => `The person checked \`${path}\` and changed nothing.`;

  return { dotenvSaved, dotenvChecked, textSaved, textChecked, mayName };
});
