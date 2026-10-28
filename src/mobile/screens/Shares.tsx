import { useEffect } from "react";
import type { Share } from "../api";
import { EmptyState, PcChip } from "../components/Chrome";
import { Icon } from "../components/Icon";
import { ago, fileSize } from "../format";
import { locale, t } from "../i18n";
import { isImage } from "../model";
import { useRemoteContext } from "../remote";

/** What agents shared with the phone, newest first: images shown, files to open. */
export function Shares({ now, onSeen }: { now: number; onSeen: (at: number) => void }) {
  const { shares, client, pc, connection } = useRemoteContext();
  const newest = shares[0]?.at;

  // Seen as soon as they are on screen.
  useEffect(() => {
    if (newest !== undefined) onSeen(newest);
  }, [newest, onSeen]);

  return (
    <div className="page">
      <div className="top">
        <h1 className="title">{t("shares.title")}</h1>
        <PcChip pc={pc} connection={connection} />
      </div>
      {shares.length === 0 ? (
        <EmptyState title={t("shares.empty")} text={t("shares.emptyHint")} mark={<Icon name="image" size={40} />} />
      ) : (
        <ul className="stack section">
          {shares.map((share) => (
            <li key={share.id} className="share">
              {isImage(share) ? <Picture share={share} url={client.fileUrl(share.id)} now={now} /> : <File share={share} url={client.fileUrl(share.id)} now={now} />}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

const from = (share: Share, now: number) =>
  `${t("shares.from", { pane: share.pane, workspace: share.workspace })}, ${ago(share.at, now, locale)}`;

/** Who shared it and when, and the one thing to do: open it. */
function Foot({ share, url, now }: { share: Share; url: string; now: number }) {
  return (
    <div className="share__foot">
      <span className="share__meta">{from(share, now)}</span>
      <a className="btn" href={url} target="_blank" rel="noopener" aria-label={`${t("shares.open")} ${share.name}`}>
        {t("shares.open")}
      </a>
    </div>
  );
}

function Picture({ share, url, now }: { share: Share; url: string; now: number }) {
  return (
    <figure>
      <a className="share__image" href={url} target="_blank" rel="noopener" tabIndex={-1} aria-hidden="true">
        <img src={url} alt={share.caption ?? share.name} loading="lazy" decoding="async" />
      </a>
      <figcaption className="share__body">
        <p className="share__caption">{share.caption ?? share.name}</p>
        <Foot share={share} url={url} now={now} />
      </figcaption>
    </figure>
  );
}

function File({ share, url, now }: { share: Share; url: string; now: number }) {
  const extension = /\.([a-z0-9]{1,5})$/i.exec(share.name)?.[1]?.toUpperCase() ?? "";
  return (
    <div className="share__body">
      <div className="file">
        <span className="file__type" aria-hidden="true">
          {extension}
        </span>
        <span className="file__main">
          <span className="file__name">{share.name}</span>
          {share.caption && <span className="file__caption">{share.caption}</span>}
          <span className="file__size">{fileSize(share.size, locale)}</span>
        </span>
      </div>
      <Foot share={share} url={url} now={now} />
    </div>
  );
}
