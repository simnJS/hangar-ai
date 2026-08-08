import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { voiceRecording, voiceStart, voiceStop } from "./ipc";
import type { Settings, VoiceEngine } from "../types";

/**
 * Dictation, from the interface's side.
 *
 * The Rust half owns the microphone and the model; this owns the gesture, and
 * the gesture is push-to-talk and nothing else: the key is down, it listens;
 * the key comes up, it transcribes. Nothing to learn and nothing to leave
 * running by accident — the microphone cannot be open unless a finger is on it.
 *
 * That the shortcut only ever starts is what makes it safe. Holding a key
 * repeats its keydown, so a press that also stopped would cut every dictation
 * short at the auto-repeat delay.
 */

export type VoicePhase = "idle" | "recording" | "transcribing";

/** What the Rust side is given for a dictation. Mirrors `VoiceConfig`. */
export interface VoiceConfig {
  engine: VoiceEngine;
  model: string;
  language: string | null;
  apiKey: string;
  cleanup: boolean;
  cleanupModel: string;
  cleanupHint: string;
}

/** One entry of the local model catalogue. Mirrors `ModelInfo`. */
export interface VoiceModel {
  id: string;
  label: string;
  approxBytes: number;
  languages: string;
  license: string;
  installed: boolean;
}

/** Download progress for a model. Mirrors `Progress`. */
export interface VoiceDownload {
  id: string;
  received: number;
  total: number;
  done: boolean;
}

export function voiceConfig(settings: Settings): VoiceConfig {
  const local = settings.voiceEngine === "local";
  return {
    engine: settings.voiceEngine,
    model: local ? settings.voiceModel : settings.voiceCloudModel,
    language: settings.voiceLanguage || null,
    apiKey: settings.voiceApiKey,
    cleanup: settings.voiceCleanup,
    cleanupModel: settings.voiceCleanupModel,
    cleanupHint: settings.voiceCleanupHint,
  };
}

export function useVoice(options: {
  settings: Settings;
  /** Receives the finished transcript, and decides where it lands. */
  onText: (text: string) => void;
  /** Shown as a toast; also how a missing microphone is reported. */
  onError: (message: string) => void;
}) {
  const { settings, onText, onError } = options;

  const [phase, setPhase] = useState<VoicePhase>("idle");
  const [level, setLevel] = useState(0);

  // The listener below is installed once and reads everything through refs, so
  // a settings change mid-recording cannot swap the handler under it.
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const onTextRef = useRef(onText);
  onTextRef.current = onText;
  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;

  const phaseRef = useRef<VoicePhase>("idle");

  const setPhaseBoth = useCallback((next: VoicePhase) => {
    phaseRef.current = next;
    setPhase(next);
    if (next !== "recording") setLevel(0);
  }, []);

  useEffect(() => {
    const unlisten: Array<() => void> = [];
    let cancelled = false;

    const track = (promise: Promise<() => void>) =>
      promise
        .then((off) => {
          // The effect may have been torn down while the listener was being
          // registered; dropping it on the floor would leak it for good.
          if (cancelled) off();
          else unlisten.push(off);
        })
        .catch(() => undefined);

    track(
      listen<{ phase: VoicePhase; error: string | null }>("voice:phase", (event) => {
        setPhaseBoth(event.payload.phase);
        if (event.payload.error) onErrorRef.current(event.payload.error);
      }),
    );
    track(
      listen<{ level: number }>("voice:level", (event) => setLevel(event.payload.level)),
    );
    track(
      listen<{ text: string }>("voice:text", (event) => {
        if (event.payload.text) onTextRef.current(event.payload.text);
      }),
    );

    return () => {
      cancelled = true;
      unlisten.forEach((off) => off());
    };
  }, [setPhaseBoth]);

  /**
   * A reload leaves the Rust side holding an open microphone that no gesture
   * can close any more — the key that started it belongs to a window that no
   * longer exists. Dropped rather than adopted: the audio recorded before the
   * reload is not something anyone still wants typed into a pane.
   */
  useEffect(() => {
    voiceRecording()
      .then((live) => {
        if (live) voiceStop(voiceConfig(settingsRef.current), true).catch(() => undefined);
      })
      .catch(() => undefined);
  }, []);

  const stop = useCallback(
    (cancel: boolean) => {
      if (phaseRef.current !== "recording") return;
      // Moved out of `recording` here rather than on the event coming back, so
      // a second press during the round trip cannot start a new recording.
      setPhaseBoth(cancel ? "idle" : "transcribing");
      voiceStop(voiceConfig(settingsRef.current), cancel).catch(() => undefined);
    },
    [setPhaseBoth],
  );

  /**
   * What the shortcut runs, and the only thing it does: start.
   *
   * Every later press while the key is still down is the keyboard repeating
   * itself, and a dictation still being transcribed is not something to queue
   * another one behind — it would race the first one into the same pane.
   */
  const press = useCallback(() => {
    if (phaseRef.current !== "idle") return;

    setPhaseBoth("recording");
    voiceStart(voiceConfig(settingsRef.current)).catch((err) => {
      setPhaseBoth("idle");
      onErrorRef.current(String(err));
    });
  }, [setPhaseBoth]);

  // Releasing, Escape, and losing the window all only mean something while a
  // recording is open.
  useEffect(() => {
    if (phase !== "recording") return;

    // Any release ends it, not just the shortcut's own key: letting go of a
    // three-key chord sends them up in whatever order the fingers leave, and
    // waiting for one in particular would keep listening through the others.
    // Nobody types while dictating, so there is nothing else this could be.
    const onKeyUp = () => stop(false);

    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      stop(true);
    }

    // A window that loses focus mid-hold — Alt+Tab, a notification stealing
    // it — never sees the release, and the microphone would stay open until
    // the length cap. Ended rather than cancelled: the words were still said.
    const onBlur = () => stop(false);

    // Capture phase for Escape, above the terminal and every dialog: cancelling
    // a dictation must not also close whatever is on screen behind it.
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [phase, stop]);

  // No `cancel` in the returned shape: Escape is handled here, above whatever
  // has the focus, and there is nowhere else it could sensibly be raised from.
  return { phase, level, press };
}
