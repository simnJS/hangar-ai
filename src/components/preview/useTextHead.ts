import { useCallback, useEffect, useState } from "react";
import { rangeTotal, TEXT_HEAD_BYTES, textHead } from "../../lib/preview";
import { errorText } from "./parts";

export type TextState =
  | { status: "loading" }
  | { status: "error"; error: string }
  | {
      status: "ready";
      text: string;
      /** The file goes on past what is drawn. */
      truncated: boolean;
      /** Bytes read, and the whole file's. */
      read: number;
      total: number;
    };

/**
 * The head of a text file — TEXT_HEAD_BYTES at most, asked for with a
 * `Range`, which the scheme honours — decoded as UTF-8 and cut to whole
 * lines (lib/preview › textHead). A file shorter than that comes whole.
 * Nothing is read while `url` is null.
 */
export function useTextHead(url: string | null): TextState & { retry: () => void } {
  const [state, setState] = useState<TextState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (url === null) return;
    const controller = new AbortController();
    setState({ status: "loading" });
    fetch(url, {
      headers: { Range: `bytes=0-${TEXT_HEAD_BYTES - 1}` },
      cache: "no-store",
      signal: controller.signal,
    })
      .then(async (response) => {
        // An empty file has no first byte to give.
        if (response.status === 416) return { text: "", read: 0, total: 0 };
        if (!response.ok) {
          const reason = await response.text().catch(() => "");
          throw new Error(reason.trim() || `HTTP ${response.status}`);
        }
        const bytes = new Uint8Array(await response.arrayBuffer());
        // A server that ignored the range sent it all: only the head is drawn all the same.
        const read = Math.min(bytes.length, TEXT_HEAD_BYTES);
        const total = rangeTotal(response.headers.get("Content-Range")) ?? bytes.length;
        return { text: new TextDecoder().decode(bytes.subarray(0, read)), read, total };
      })
      .then(({ text, read, total }) => {
        if (controller.signal.aborted) return;
        setState({ status: "ready", ...textHead(text, read < total), read, total });
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) setState({ status: "error", error: errorText(error) });
      });
    return () => controller.abort();
  }, [url, attempt]);

  const retry = useCallback(() => setAttempt((n) => n + 1), []);
  return { ...state, retry };
}
