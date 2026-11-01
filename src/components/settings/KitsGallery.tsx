import type { Translator } from "../../i18n";
import { filterGallery, galleryStatus, type Gallery } from "../../lib/kitGallery";
import type { KitInfo } from "../../lib/kits";
import "./KitsGallery.css";

/**
 * Settings › Kits › Library: the public kits of the `hangar-ai` organization, from
 * a static file Rust reads. Drawn from props alone, like `KitsView`. Installing
 * only brings the kit onto the machine; enabling it stays in the activation,
 * where its permissions are shown, verified kit or not — and the page says so.
 */

export interface KitsGalleryProps {
  t: Translator;
  /** Null until Rust has answered. */
  gallery: Gallery | null;
  /** The list could not be asked for at all. */
  error: string | null;
  refreshing: boolean;
  /** The kits on this machine; null until they are read. */
  installed: readonly KitInfo[] | null;
  /** The gallery kit being installed, or updated. */
  busy: string | null;
  /** The last install that failed. */
  failure: { id: string; name: string; detail: string } | null;
  query: string;
  onQuery: (query: string) => void;
  onRefresh: () => void;
  onInstall: (id: string) => void;
  onUpdate: (kit: KitInfo) => void;
}

export function KitsGalleryView(props: KitsGalleryProps) {
  const { t, gallery, installed, busy, failure } = props;
  const shown = gallery ? filterGallery(gallery.kits, props.query) : [];
  const date = gallery?.fetchedAt ? new Date(gallery.fetchedAt).toLocaleDateString() : "";

  return (
    <section className="set-section kit-gallery" aria-labelledby="kit-gallery-title">
      <header className="set-section__head">
        <h3 className="set-section__title" id="kit-gallery-title">
          {t("kitsManage.galleryTitle")}
        </h3>
        <p className="set-section__hint">{t("kitsManage.galleryHint")}</p>
      </header>
      <div className="set-group">
        <div className="set-row kit-gallery__bar">
          <input
            type="search"
            className="set-input kit-gallery__filter"
            value={props.query}
            placeholder={t("kitsManage.galleryFilterPlaceholder")}
            aria-label={t("kitsManage.galleryFilter")}
            spellCheck={false}
            autoComplete="off"
            onChange={(event) => props.onQuery(event.target.value)}
          />
          <button type="button" className="btn" disabled={props.refreshing} onClick={props.onRefresh}>
            {props.refreshing ? t("kitsManage.galleryRefreshing") : t("kitsManage.galleryRefresh")}
          </button>
        </div>

        {gallery?.origin === "cached" && (
          <div className="set-row">
            <p className="kit-row__note" role="status">
              {t("kitsManage.galleryCached", { date })}
            </p>
          </div>
        )}
        {(gallery?.origin === "unavailable" || props.error) && (
          <div className="set-row">
            <p className="notice" role="status">
              <span className="notice__text">{t("kitsManage.galleryUnavailable")}</span>
            </p>
          </div>
        )}
        {failure && (
          <div className="set-row">
            <p className="notice notice--error" role="alert">
              <span className="notice__text">
                {t("kitsManage.galleryInstallFailed", { name: failure.name, detail: failure.detail })}
              </span>
            </p>
          </div>
        )}

        {gallery === null && !props.error ? (
          <div className="set-row">
            <span className="set-row__hint">{t("kitsManage.galleryLoading")}</span>
          </div>
        ) : gallery && gallery.origin !== "unavailable" && gallery.kits.length === 0 ? (
          <div className="set-row">
            <span className="set-row__hint">{t("kitsManage.galleryEmpty")}</span>
          </div>
        ) : gallery && gallery.kits.length > 0 && shown.length === 0 ? (
          <div className="set-row">
            <span className="set-row__hint">{t("kitsManage.galleryNoMatch")}</span>
          </div>
        ) : (
          shown.map((entry) => {
            const status = galleryStatus(entry, installed ?? []);
            const local = installed?.find((kit) => kit.id === entry.id);
            const working = busy === entry.id;
            return (
              <div className="set-row set-row--block kit-row kit-gallery__card" key={entry.id}>
                <div className="kit-gallery__top">
                  <div className="kit-row__text">
                    <span className="set-row__label">
                      {entry.name}
                      <span className="kit-row__ver">{t("kitsManage.version", { version: entry.version })}</span>
                    </span>
                    <span className="set-row__hint">{entry.description}</span>
                    <span className="kit-row__source" title={`${entry.repo} · ${entry.tag}`}>
                      {entry.repo} · {entry.tag}
                    </span>
                  </div>
                  <div className="kit-gallery__action">
                    {status.kind === "new" && (
                      <button
                        type="button"
                        className="btn btn--primary"
                        disabled={working || busy !== null || installed === null}
                        aria-label={`${t("kitsManage.galleryInstall")} ${entry.name}`}
                        onClick={() => props.onInstall(entry.id)}
                      >
                        {working ? t("kitsManage.galleryInstalling") : t("kitsManage.galleryInstall")}
                      </button>
                    )}
                    {status.kind === "update" && local && (
                      <button
                        type="button"
                        className="btn btn--primary"
                        disabled={busy !== null}
                        aria-label={`${t("kitsManage.galleryUpdate", { latest: status.latest })} ${entry.name}`}
                        onClick={() => props.onUpdate(local)}
                      >
                        {working ? t("kitsManage.updating") : t("kitsManage.galleryUpdate", { latest: status.latest })}
                      </button>
                    )}
                  </div>
                </div>
                <p className="kit-row__note kit-gallery__line">
                  {entry.verified ? (
                    <span className="kit-act__tag kit-act__tag--ok">{t("kitsManage.galleryVerified")}</span>
                  ) : (
                    <>
                      <span className="kit-act__tag">{t("kitsManage.galleryCommunity")}</span>
                      <span>{t("kitsManage.galleryCommunityNote")}</span>
                    </>
                  )}
                </p>
                {entry.tags.length > 0 && (
                  <ul className="kit-gallery__tags" aria-label={t("kitsManage.galleryTags")}>
                    {entry.tags.map((tag) => (
                      <li key={tag} className="kit-act__tag">
                        {tag}
                      </li>
                    ))}
                  </ul>
                )}
                {entry.permissionsSummary && (
                  <p className="kit-row__note kit-gallery__asks">
                    {t("kitsManage.galleryAsks", { summary: entry.permissionsSummary })}
                  </p>
                )}
                {status.kind === "installed" && (
                  <p className="kit-row__note kit-gallery__line" role="status">
                    <span className="kit-act__tag kit-act__tag--ok">
                      {t("kitsManage.galleryInstalled", { version: status.version })}
                    </span>
                    {status.pinned && (
                      <span className="kit-act__tag">{t("kitsManage.galleryPinned", { tag: status.pinned })}</span>
                    )}
                  </p>
                )}
                {status.kind === "linked" && (
                  <p className="kit-row__note kit-gallery__line" role="status">
                    <span className="kit-act__tag">
                      {t("kitsManage.linked")} (v{status.version})
                    </span>
                  </p>
                )}
                {status.kind === "update" && (
                  <p className="kit-row__note kit-gallery__line" role="status">
                    <span className="kit-act__tag kit-act__tag--new">
                      {t("kitsManage.galleryUpdateAvailable", { version: status.version, latest: status.latest })}
                    </span>
                    <span className="kit-act__tag">{t("kitsManage.galleryPinned", { tag: status.pinned })}</span>
                  </p>
                )}
              </div>
            );
          })
        )}
      </div>
    </section>
  );
}
