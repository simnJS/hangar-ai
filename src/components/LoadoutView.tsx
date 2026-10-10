import { useCallback, useEffect, useState } from "react";
import {
  loadoutEquip,
  loadoutPresets,
  loadoutStatus,
  missingFixes,
  presetIsEquipped,
  type LoadoutPlugin,
  type LoadoutStatus,
  type Preset,
  type ProviderUse,
} from "../lib/loadout";
import { useT } from "../i18n";

interface Props {
  /** Active workspace: the project whose `.claude/settings.json` is read and
      equipped. Key the view on it: its state belongs to one workspace. */
  cwd: string;
}

/**
 * Last status per workspace, so coming back to the tab draws at once while the
 * engine checks again — a check asks GitHub about private plugins and takes
 * seconds.
 */
const lastStatus = new Map<string, LoadoutStatus>();

const STATE_GLYPH: Record<ProviderUse["state"], string> = {
  ok: "✓",
  missing: "✕",
  session: "○",
};

function ProviderLine({ use, required }: { use: ProviderUse; required: boolean }) {
  const t = useT();
  return (
    <li className={`lo-use lo-use--${use.state}`}>
      <span className="lo-use__glyph" aria-hidden>
        {STATE_GLYPH[use.state]}
      </span>
      <span className="lo-use__label">
        {required ? t("loadout.needs", { label: use.label }) : use.label}
      </span>
      {use.ask_before && <em className="lo-badge lo-badge--paid">{t("loadout.asksFirst")}</em>}
      {use.cost && <span className="lo-use__cost">{use.cost}</span>}
      {use.state === "missing" && <span className="lo-use__detail">{use.detail}</span>}
      {use.state === "session" && <span className="lo-use__detail">{t("loadout.session")}</span>}
    </li>
  );
}

function PluginCard({ plugin }: { plugin: LoadoutPlugin }) {
  const t = useT();
  return (
    <section className={`lo-plugin ${plugin.equipped ? "lo-plugin--equipped" : ""}`}>
      <header className="lo-plugin__head">
        <h3 className="lo-plugin__name">{plugin.name}</h3>
        {!plugin.access ? (
          <em className="lo-badge lo-badge--muted">🔒 {t("loadout.locked")}</em>
        ) : plugin.equipped ? (
          <em className="lo-badge">{t("loadout.equipped")}</em>
        ) : (
          <em className="lo-badge lo-badge--muted">{t("loadout.available")}</em>
        )}
      </header>
      {plugin.access &&
        plugin.capabilities.map((capability) => (
          <div key={capability.id} className="lo-cap">
            <p className={`lo-cap__label ${capability.ready ? "is-ready" : "is-blocked"}`}>
              <span className="lo-cap__dot" aria-hidden />
              {capability.label}
            </p>
            <ul className="lo-cap__uses">
              {capability.requires.map((use) => (
                <ProviderLine key={`r-${use.provider}`} use={use} required />
              ))}
              {capability.chain.map((use) => (
                <ProviderLine key={`c-${use.provider}`} use={use} required={false} />
              ))}
            </ul>
          </div>
        ))}
      {!plugin.access && <p className="lo-plugin__note">{t("loadout.lockedHint")}</p>}
    </section>
  );
}

export function LoadoutView({ cwd }: Props) {
  const t = useT();
  const [status, setStatus] = useState<LoadoutStatus | null>(() => lastStatus.get(cwd) ?? null);
  const [presets, setPresets] = useState<Record<string, Preset>>({});
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [preset, setPreset] = useState("");
  const [equipping, setEquipping] = useState(false);
  const [equipped, setEquipped] = useState<string | null>(null);

  const refresh = useCallback(() => {
    setChecking(true);
    setError(null);
    loadoutStatus(cwd)
      .then((next) => {
        lastStatus.set(cwd, next);
        setStatus(next);
      })
      .catch((err) => setError(String(err)))
      .finally(() => setChecking(false));
  }, [cwd]);

  // Mounted once per workspace (keyed on it by the caller), so a check still
  // running for the previous one cannot land here.
  useEffect(refresh, [refresh]);

  useEffect(() => {
    loadoutPresets()
      .then(setPresets)
      .catch(() => setPresets({}));
  }, []);

  async function equip() {
    if (!preset) return;
    setEquipping(true);
    setError(null);
    try {
      const report = await loadoutEquip(cwd, preset);
      setEquipped(report.plugins.join(", "));
      refresh();
    } catch (err) {
      setError(String(err));
    } finally {
      setEquipping(false);
    }
  }

  const fixes = status ? missingFixes(status) : [];
  const chosen = presets[preset];

  return (
    <div className="loadout">
      <div className="loadout__bar">
        <span className="loadout__title">{t("loadout.title")}</span>
        <select
          className="loadout__preset"
          value={preset}
          onChange={(e) => setPreset(e.target.value)}
          aria-label={t("loadout.preset")}
        >
          <option value="">{t("loadout.choosePreset")}</option>
          {Object.entries(presets).map(([id, p]) => (
            <option key={id} value={id}>
              {p.label} — {p.plugins.join(", ")}
              {status && presetIsEquipped(p, status) ? ` ✓` : ""}
            </option>
          ))}
        </select>
        <button
          className="btn btn--primary"
          onClick={equip}
          disabled={!preset || equipping}
          title={chosen ? t("loadout.equipHint") : undefined}
        >
          {equipping ? t("loadout.equipping") : t("loadout.equip")}
        </button>
        <span className="loadout__hint">{checking ? t("loadout.checking") : ""}</span>
        <button className="btn btn--ghost" onClick={refresh} disabled={checking}>
          {t("loadout.refresh")}
        </button>
      </div>

      {error && <p className="loadout__error">{error}</p>}
      {equipped && (
        <p className="loadout__notice">{t("loadout.equippedNotice", { plugins: equipped })}</p>
      )}

      <div className="loadout__body">
        {!status && !error && <p className="loadout__none">{t("loadout.firstCheck")}</p>}

        {fixes.length > 0 && (
          <section className="lo-fixes">
            <h3>{t("loadout.fixes")}</h3>
            <ul>
              {fixes.map((fix) => (
                <li key={fix.provider}>
                  <strong>{fix.label}</strong> — {fix.how}
                </li>
              ))}
            </ul>
          </section>
        )}

        {status && (
          <div className="loadout__plugins">
            {status.plugins.map((plugin) => (
              <PluginCard key={plugin.name} plugin={plugin} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
