import { useEffect, useMemo, useRef, useState } from "react";
import { useT, type TranslateKey } from "../../i18n";
import { useStore } from "../../store";
import {
  chordFromEvent,
  chordSegments,
  formatBinding,
  isBareChord,
  isMac,
  parseBinding,
  writeBinding,
  type Binding,
  type Chord,
} from "../../lib/keys";
import {
  bindingsOf,
  commandLabel,
  commandsFor,
  COMMANDS,
  COMMANDS_BY_ID,
  findConflicts,
  isCustomized,
  resetCommand,
  resolveKeymap,
  SECTIONS,
  withBinding,
  withoutBinding,
  type CommandId,
  type Keymap,
  type ShortcutCommand,
  type ShortcutSection,
} from "../../lib/shortcuts";
import { suspendShortcuts } from "../../lib/useShortcuts";

/**
 * The keyboard shortcut editor.
 *
 * Deliberately self-contained — it reads the store and nothing else — so it
 * drops into the settings page as one block, among controls that are all
 * label-plus-widget rows and could not describe this one.
 */

/** Search ignores case and accents, like the settings page around it. */
const fold = (text: string) =>
  text
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase();

const SECTION_KEYS: Record<ShortcutSection, TranslateKey> = {
  panes: "keymap.section.panes",
  navigation: "keymap.section.navigation",
  workspaces: "keymap.section.workspaces",
  view: "keymap.section.view",
  terminal: "keymap.section.terminal",
  voice: "keymap.section.voice",
  app: "keymap.section.app",
};

/** A binding drawn as the keys it is made of. */
function Keys({ binding }: { binding: Binding }) {
  const chords = parseBinding(binding);
  if (!chords) return <span className="kbd-seq">{binding}</span>;
  return (
    <span className="kbd-seq">
      {chords.map((chord, i) => (
        <span className="kbd-chord" key={i}>
          {chordSegments(chord).map((segment, j) => (
            <kbd key={j}>{segment}</kbd>
          ))}
        </span>
      ))}
    </span>
  );
}

interface Held {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  meta: boolean;
}

/**
 * The modifiers currently down, spelled the way the rest of the app spells
 * them — ⌃⌥⇧⌘ on a Mac — so what the recorder shows while a chord is being
 * held matches what it shows once the chord is recorded.
 */
const heldSegments = (held: Held) =>
  (isMac()
    ? [held.ctrl && "⌃", held.alt && "⌥", held.shift && "⇧", held.meta && "⌘"]
    : [held.ctrl && "Ctrl", held.alt && "Alt", held.shift && "Shift", held.meta && "Win"]
  ).filter((segment): segment is string => Boolean(segment));

/**
 * Captures keystrokes until the user confirms.
 *
 * Enter and Escape on their own steer the recorder rather than being recorded,
 * which is the one thing it cannot bind — held with any modifier they are
 * ordinary keys again, so Ctrl+Shift+Enter stays reachable. That is what buys
 * sequences: every other chord is appended, so Ctrl+K then Ctrl+S records as
 * a single binding.
 */
function Recorder({
  onDone,
  onCancel,
}: {
  onDone: (chords: Chord[]) => void;
  onCancel: () => void;
}) {
  const [chords, setChords] = useState<Chord[]>([]);
  const [held, setHeld] = useState<Held | null>(null);

  const t = useT();

  // The listener is installed once — reinstalling it would drop and retake the
  // dispatcher's suspension on every keystroke — so everything it reads or
  // calls goes through a ref.
  const chordsRef = useRef<Chord[]>([]);
  chordsRef.current = chords;
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  const cancelRef = useRef(onCancel);
  cancelRef.current = onCancel;

  useEffect(() => {
    const release = suspendShortcuts();

    function onKeyDown(event: KeyboardEvent) {
      event.preventDefault();
      event.stopPropagation();

      const chord = chordFromEvent(event);
      if (!chord) {
        // Modifiers alone are not a chord, but showing them while they are
        // held is what makes the recorder feel like it is listening.
        const down = {
          ctrl: event.ctrlKey,
          shift: event.shiftKey,
          alt: event.altKey,
          meta: event.metaKey,
        };
        setHeld(heldSegments(down).length ? down : null);
        return;
      }

      if (isBareChord(chord) && !chord.shift) {
        if (chord.key === "Escape") {
          cancelRef.current();
          return;
        }
        if (chord.key === "Enter") {
          if (chordsRef.current.length) doneRef.current(chordsRef.current);
          return;
        }
      }

      setHeld(null);
      // Two chords is as long as a sequence gets; a third starts over rather
      // than growing something nobody could type in time.
      setChords((current) => (current.length >= 2 ? [chord] : [...current, chord]));
    }

    const onKeyUp = () => setHeld(null);

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      release();
    };
  }, []);

  return (
    <span className="key-rec">
      <span className="key-rec__dot" aria-hidden="true" />

      {chords.length > 0 && <Keys binding={writeBinding(chords)} />}

      {chords.length === 0 &&
        (held ? (
          <span className="kbd-seq">
            {heldSegments(held).map((segment, i) => (
              <kbd key={i}>{segment}</kbd>
            ))}
            <kbd className="kbd--ghost">…</kbd>
          </span>
        ) : (
          <span className="key-rec__prompt">{t("keymap.recording")}</span>
        ))}

      <span className="key-rec__hint">{t("keymap.recordingHint")}</span>

      <button
        type="button"
        className="key-row__btn"
        title={t("keymap.confirm")}
        disabled={!chords.length}
        onClick={() => onDone(chords)}
      >
        ✓
      </button>
      <button
        type="button"
        className="key-row__btn"
        title={t("keymap.cancel")}
        onClick={onCancel}
      >
        ✕
      </button>
    </span>
  );
}

function CommandRow({
  command,
  keymap,
  overrides,
  recording,
  onRecord,
  onChange,
}: {
  command: ShortcutCommand;
  keymap: Keymap;
  overrides: Keymap;
  recording: boolean;
  onRecord: (id: CommandId | null) => void;
  onChange: (next: Keymap) => void;
}) {
  const t = useT();
  const bindings = bindingsOf(keymap, command.id);
  const custom = isCustomized(overrides, command.id);

  /** Other commands answering to the same keystrokes as this one. */
  const rivals = useMemo(() => {
    const found = new Map<Binding, CommandId[]>();
    for (const binding of bindings) {
      const chords = parseBinding(binding);
      if (!chords) continue;
      const others = commandsFor(keymap, chords, command.id);
      if (others.length) found.set(binding, others);
    }
    return found;
  }, [bindings, keymap, command.id]);

  /** Takes a keystroke away from whoever else answers to it. */
  function free(binding: Binding, others: CommandId[]) {
    let next = overrides;
    for (const id of others) next = withoutBinding(next, keymap, id, binding);
    onChange(next);
  }

  return (
    <li className={`key-row ${rivals.size ? "is-conflict" : ""}`}>
      <span className="key-row__text">
        <span className="key-row__label">{commandLabel(t, command)}</span>
        {custom && <span className="key-row__tag">{t("keymap.custom")}</span>}
      </span>

      <span className="key-row__binds">
        {bindings.map((binding) => (
          <span
            key={binding}
            className={`key-bind ${rivals.has(binding) ? "is-conflict" : ""}`}
          >
            <Keys binding={binding} />
            <button
              type="button"
              className="key-bind__x"
              title={t("keymap.remove", { keys: formatBinding(binding) })}
              onClick={() =>
                onChange(withoutBinding(overrides, keymap, command.id, binding))
              }
            >
              ×
            </button>
          </span>
        ))}

        {!bindings.length && !recording && (
          <span className="key-row__none">{t("keymap.unbound")}</span>
        )}

        {recording ? (
          <Recorder
            onCancel={() => onRecord(null)}
            onDone={(chords) => {
              onChange(withBinding(overrides, keymap, command.id, writeBinding(chords)));
              onRecord(null);
            }}
          />
        ) : (
          <>
            <button
              type="button"
              className="key-row__btn"
              title={t("keymap.add")}
              onClick={() => onRecord(command.id)}
            >
              +
            </button>
            <button
              type="button"
              className="key-row__btn"
              title={t("keymap.reset")}
              disabled={!custom}
              onClick={() => onChange(resetCommand(overrides, command.id))}
            >
              ↺
            </button>
          </>
        )}
      </span>

      {[...rivals].map(([binding, others]) => (
        <span className="key-row__warn" key={binding}>
          {t("keymap.alsoBound", {
            keys: formatBinding(binding),
            commands: others
              .map((id) => {
                const other = COMMANDS_BY_ID.get(id);
                return other ? commandLabel(t, other) : id;
              })
              .join(", "),
          })}
          <button
            type="button"
            className="key-row__link"
            onClick={() => free(binding, others)}
          >
            {t("keymap.free")}
          </button>
        </span>
      ))}
    </li>
  );
}

export function ShortcutsSettings() {
  const { state, updateSettings } = useStore();
  const t = useT();
  const [query, setQuery] = useState("");
  const [recording, setRecording] = useState<CommandId | null>(null);
  const [confirmReset, setConfirmReset] = useState(false);

  const overrides = state.settings.keybindings ?? {};
  const keymap = useMemo(() => resolveKeymap(overrides), [overrides]);
  const conflicts = useMemo(() => findConflicts(keymap), [keymap]);

  const apply = (next: Keymap) => updateSettings({ keybindings: next });

  const needle = fold(query.trim());
  const matches = (command: ShortcutCommand) => {
    if (!needle) return true;
    const haystack = [
      commandLabel(t, command),
      ...bindingsOf(keymap, command.id).map(formatBinding),
      t(SECTION_KEYS[command.section]),
    ].join(" ");
    return fold(haystack).includes(needle);
  };

  const groups = SECTIONS.map((section) => ({
    section,
    commands: COMMANDS.filter(
      (command) => command.section === section && matches(command),
    ),
  })).filter((group) => group.commands.length > 0);

  const customCount = COMMANDS.filter((command) =>
    isCustomized(overrides, command.id),
  ).length;

  return (
    <div className="keymap">
      <div className="keymap__bar">
        <div className="keymap__search">
          <span className="keymap__search-icon" aria-hidden="true">
            ⌕
          </span>
          <input
            value={query}
            placeholder={t("keymap.search")}
            aria-label={t("keymap.search")}
            onChange={(e) => setQuery(e.target.value)}
            // The recorder listens on the window, so anything typed here would
            // be read as the shortcut instead of reaching the field.
            onFocus={() => setRecording(null)}
          />
          {query && (
            <button
              type="button"
              className="icon-btn"
              title={t("keymap.clear")}
              onClick={() => setQuery("")}
            >
              ×
            </button>
          )}
        </div>

        <span
          className={`keymap__state ${conflicts.length ? "is-conflict" : ""}`}
          title={conflicts.map((conflict) => formatBinding(conflict.binding)).join(" · ")}
        >
          {conflicts.length
            ? t("keymap.conflicts", { n: conflicts.length })
            : t("keymap.noConflict")}
        </span>

        {confirmReset ? (
          <>
            <button
              type="button"
              className="btn btn--ghost"
              onClick={() => setConfirmReset(false)}
            >
              {t("create.cancel")}
            </button>
            <button
              type="button"
              className="btn btn--danger"
              onClick={() => {
                apply({});
                setConfirmReset(false);
              }}
            >
              {t("keymap.resetAllConfirm")}
            </button>
          </>
        ) : (
          <button
            type="button"
            className="btn"
            disabled={!customCount}
            title={t("keymap.resetAllHint")}
            onClick={() => setConfirmReset(true)}
          >
            {t("keymap.resetAll")}
          </button>
        )}
      </div>

      {groups.map(({ section, commands }) => (
        <div className="keymap__group" key={section}>
          <h5 className="keymap__group-title">{t(SECTION_KEYS[section])}</h5>
          <ul className="keymap__list">
            {commands.map((command) => (
              <CommandRow
                key={command.id}
                command={command}
                keymap={keymap}
                overrides={overrides}
                recording={recording === command.id}
                onRecord={setRecording}
                onChange={apply}
              />
            ))}
          </ul>
        </div>
      ))}

      {!groups.length && <p className="keymap__empty">{t("keymap.empty")}</p>}
    </div>
  );
}
