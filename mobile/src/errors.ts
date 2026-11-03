import { ApiError } from "./api";
import { t } from "./i18n";

/** What went wrong, said to the person: what happened and what to do. */
export function errorText(error: unknown): string {
  if (!(error instanceof ApiError)) return t("error.offline");
  switch (error.kind) {
    case "offline":
      return t("error.offline");
    case "busy":
      return t("error.busy");
    case "unpaired":
      return t("pair.again");
    case "refused":
      return error.code === "too-big" ? t("error.tooBig") : t("error.refused", { reason: error.detail ?? error.code });
  }
}
