import { useState } from "react";
import { ApiError } from "../api";
import { Logo } from "../components/Icon";
import { errorText } from "../errors";
import { t } from "../i18n";
import { digitsOf } from "../model";
import { useRemoteContext } from "../remote";

/**
 * Pairing: the code the PC shows, typed here. Scanning the PC's QR code opens
 * the app with the code in the address, which pairs by itself; this screen
 * shows only if that did not work, or the phone came here on its own.
 */
export function Pairing() {
  const { pair, lostPairing, codeRefused } = useRemoteContext();
  const [code, setCode] = useState("");
  const [pairing, setPairing] = useState(false);
  const [error, setError] = useState<string | null>(() => (codeRefused ? refusal(codeRefused) : null));

  const submit = async (value: string) => {
    if (value.length !== 6 || pairing) return;
    setPairing(true);
    setError(null);
    try {
      await pair(value);
    } catch (failure) {
      setError(refusal(failure));
      setCode("");
    } finally {
      setPairing(false);
    }
  };

  return (
    <div className="page pairing">
      <Logo size={44} />
      <h1 className="title">{t("pair.title")}</h1>
      {lostPairing && <p className="pairing__lost">{t("pair.again")}</p>}
      <p className="pairing__hint">{t("pair.hint")}</p>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit(code);
        }}
      >
        <label className="sr-only" htmlFor="pair-code">
          {t("pair.code")}
        </label>
        <input
          id="pair-code"
          className="code"
          inputMode="numeric"
          autoComplete="one-time-code"
          pattern="[0-9]*"
          placeholder="••••••"
          value={code}
          disabled={pairing}
          onChange={(event) => {
            const digits = digitsOf(event.target.value);
            setCode(digits);
            setError(null);
            // The sixth digit pairs: no button to look for.
            if (digits.length === 6) void submit(digits);
          }}
        />
        <button type="submit" className="btn btn--primary btn--wide" disabled={code.length !== 6 || pairing}>
          {pairing ? t("pair.pairing") : t("pair.submit")}
        </button>
        {error && (
          <p className="pairing__error" role="alert">
            {error}
          </p>
        )}
      </form>
    </div>
  );
}

/** A refused code says so; the PC away or busy says that instead. */
function refusal(error: unknown): string {
  if (error instanceof ApiError && (error.kind === "refused" || error.kind === "unpaired")) return t("pair.refused");
  return errorText(error);
}
