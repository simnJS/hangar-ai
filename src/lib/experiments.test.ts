import { describe, expect, it } from "vitest";
import {
  availableExperiments,
  currentPlatform,
  EXPERIMENTS,
  isExperimentEnabled,
  withExperiment,
  type Experiment,
} from "./experiments";

const everywhere: Experiment = {
  id: "demo",
  stage: "experimental",
  title: "experiments.title",
  hint: "experiments.warning",
};
const linuxOnly: Experiment = { ...everywhere, id: "linux-demo", platforms: ["linux"] };
const registry = [everywhere, linuxOnly];

describe("currentPlatform", () => {
  it("reads the platform off the user agent", () => {
    expect(currentPlatform("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe("linux");
    expect(currentPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/131.0")).toBe("windows");
    expect(currentPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("macos");
  });

  it("does not take a phone for a desktop", () => {
    expect(currentPlatform("Mozilla/5.0 (Linux; Android 14; Pixel 8) Mobile")).toBeNull();
    expect(currentPlatform("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)")).toBeNull();
    expect(currentPlatform("")).toBeNull();
  });
});

describe("availableExperiments", () => {
  it("offers a feature with no platform list everywhere", () => {
    expect(availableExperiments(registry, "windows")).toEqual([everywhere]);
  });

  it("offers a platform-bound feature only there", () => {
    expect(availableExperiments(registry, "linux")).toEqual(registry);
  });

  it("offers only the unrestricted ones when the platform is unknown", () => {
    expect(availableExperiments(registry, null)).toEqual([everywhere]);
  });
});

describe("isExperimentEnabled", () => {
  it("is off for someone who never turned anything on", () => {
    expect(isExperimentEnabled({ experiments: [] }, "demo", registry, "linux")).toBe(false);
  });

  it("is on when the id is listed and registered", () => {
    expect(isExperimentEnabled({ experiments: ["demo"] }, "demo", registry, "linux")).toBe(true);
  });

  it("is off when the machine cannot run it, even if the id is listed", () => {
    const settings = { experiments: ["linux-demo"] };
    expect(isExperimentEnabled(settings, "linux-demo", registry, "linux")).toBe(true);
    expect(isExperimentEnabled(settings, "linux-demo", registry, "macos")).toBe(false);
  });

  it("ignores an id that no version registers any more", () => {
    expect(isExperimentEnabled({ experiments: ["gone"] }, "gone", registry, "linux")).toBe(false);
  });

  it("survives a settings file that holds something else than a list", () => {
    const broken = { experiments: "demo" } as unknown as { experiments: string[] };
    expect(isExperimentEnabled(broken, "demo", registry, "linux")).toBe(false);
  });
});

describe("withExperiment", () => {
  it("adds an id once, after the ones already on", () => {
    expect(withExperiment(["a"], "b", true)).toEqual(["a", "b"]);
    expect(withExperiment(["a", "b"], "a", true)).toEqual(["b", "a"]);
  });

  it("removes an id and leaves the others", () => {
    expect(withExperiment(["a", "b"], "a", false)).toEqual(["b"]);
    expect(withExperiment(["a"], "zzz", false)).toEqual(["a"]);
  });

  it("starts from nothing when the stored value is not a list", () => {
    expect(withExperiment(undefined as unknown as string[], "a", true)).toEqual(["a"]);
  });
});

describe("the shipped registry", () => {
  it("has unique, dash-separated ids", () => {
    const ids = EXPERIMENTS.map((experiment) => experiment.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z0-9]+(-[a-z0-9]+)*$/);
  });
});
