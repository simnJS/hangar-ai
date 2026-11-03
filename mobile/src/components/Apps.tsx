import type { App } from "../api";
import { appChip, appDetail, appName } from "../apps";
import { i18n, t } from "../i18n";
import { Link, useNavigation } from "./Chrome";
import { Icon } from "./Icon";

/**
 * « Voir l'app » on a project's card, or on a worker's header beside
 * Terminal: the app itself when there is one, the project's list when there
 * are several, nothing when no server listens.
 */
export function AppButton({ apps, projectId, variant }: { apps: App[]; projectId: string; variant: "card" | "head" }) {
  const { navigate } = useNavigation();
  const chip = appChip(apps);
  if (!chip) return null;
  const go = () =>
    chip.kind === "one"
      ? navigate({ name: "app", workspace: chip.app.workspaceId, port: chip.app.port })
      : navigate({ name: "project", id: projectId });
  const label = chip.kind === "one" ? t("apps.see") : t("apps.seeMany", { n: chip.count });
  const hint = chip.kind === "one" ? t("apps.open", { name: appName(chip.app, i18n), port: chip.app.port }) : label;
  return (
    <button type="button" className={variant === "head" ? "head-action head-action--app" : "app-chip"} onClick={go} aria-label={hint}>
      <Icon name="app" />
      <span>{label}</span>
    </button>
  );
}

/** A project's apps running, each opening in the viewer. */
export function AppList({ apps }: { apps: App[] }) {
  if (apps.length === 0) return null;
  return (
    <section className="section">
      <div className="section__head">
        <h2 className="section__title">{t("apps.title")}</h2>
        <span className="section__count">{apps.length}</span>
      </div>
      <p className="section__hint">{t("apps.hint")}</p>
      <ul className="list">
        {apps.map((app) => (
          <li key={`${app.workspaceId}:${app.port}`}>
            <Link
              to={{ name: "app", workspace: app.workspaceId, port: app.port }}
              className="row"
              label={t("apps.open", { name: appName(app, i18n), port: app.port })}
            >
              <Icon name="app" />
              <span className="row__main">
                <span className="row__title">
                  {appName(app, i18n)}
                  <span className="role">{t("apps.port", { port: app.port })}</span>
                </span>
                <span className="row__sub">{appDetail(app, i18n)}</span>
              </span>
              <Icon name="chevronRight" className="chevron" />
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
