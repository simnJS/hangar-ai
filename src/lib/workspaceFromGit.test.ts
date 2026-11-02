import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

import {
  CONSENT_TIMEOUT_MS,
  cloneDestination,
  consentTrace,
  createConsentQueue,
  folderName,
  recordConsent,
  firstMessage,
  fromGitButton,
  fromGitFieldsDisabled,
  fromGitKey,
  fromGitStatus,
  joinFolder,
  oneLine,
  parentFolder,
  projectsFolderOf,
  resolveParent,
  sameFolder,
  workspaceFromGit,
  type ConsentAnswer,
  type ConsentRequest,
  type FromGitDeps,
  type FromGitFormState,
  type FromGitRequest,
  type WorkspaceRef,
} from "./workspaceFromGit";
import type { CloneResult, Ownership, RepoParse } from "./gitClone";

/** A repository's address read the way Rust reads it, for the forms the tests use. */
function parseLike(input: string): RepoParse {
  const found = /^(?:https:\/\/github\.com\/)?([\w-]+)\/([\w.-]+?)(?:\.git)?$/.exec(input);
  if (!found) return { status: "refused", reason: `"${input}" is not a repository address` };
  return {
    status: "ok",
    url: `https://github.com/${found[1]}/${found[2]}.git`,
    host: "github.com",
    owner: found[1],
    name: found[2],
  };
}

interface Options {
  mine?: boolean;
  ownershipFails?: boolean;
  answer?: ConsentAnswer;
  projectsFolder?: string | null;
  existing?: WorkspaceRef | null;
  clone?: CloneResult;
  cloneThrows?: boolean;
  createFails?: boolean;
  sendFails?: boolean;
  discard?: boolean;
  discardFails?: boolean;
  removeFails?: boolean;
  noTrust?: boolean;
  trustFails?: boolean;
}

/** Fake dependencies and the log of everything that reached them. */
function fakes(options: Options = {}) {
  const log: string[] = [];
  const consents: ConsentRequest[] = [];
  const messages: string[] = [];
  const deps: FromGitDeps = {
    parse: async (input) => {
      log.push(`parse:${input}`);
      return parseLike(input);
    },
    ownership: async (owner, host) => {
      log.push(`ownership:${owner}@${host}`);
      if (options.ownershipFails) throw new Error("gh is not signed in");
      const mine = options.mine ?? false;
      return { mine, reason: mine ? "you own it" : "it belongs to someone else" } satisfies Ownership;
    },
    projectsFolder: () => (options.projectsFolder === undefined ? null : options.projectsFolder),
    findWorkspace: (folder) => {
      log.push(`find:${folder}`);
      return options.existing ?? null;
    },
    consent: async (request) => {
      log.push("consent");
      consents.push(request);
      return options.answer ?? "allow";
    },
    clone: async (url, parent, name) => {
      log.push(`clone:${url}:${parent}:${name}`);
      if (options.cloneThrows) throw new Error("ipc is gone");
      return (
        options.clone ?? {
          status: "cloned",
          path: `${parent}\\${name}`,
          reused: false,
          branch: "main",
        }
      );
    },
    newJobId: () => "job-1",
    newConsentId: () => "consent-1",
    discardClone: options.discard
      ? async (path) => {
          log.push(`discard:${path}`);
          if (options.discardFails) throw new Error("a file is in use");
        }
      : undefined,
    createWorkspace: ({ name, cwd, activate }) => {
      log.push(`workspace:${name}:${cwd}:${activate ? "front" : "behind"}`);
      if (options.createFails) throw new Error("no room for another workspace");
      const orchestrator = { id: "p-orch", name: "Kai" };
      return { workspace: { id: "w-new", name, cwd, orchestrator }, orchestrator };
    },
    trustFolder: options.noTrust
      ? undefined
      : async (path) => {
          log.push(`trust:${path}`);
          if (options.trustFails) throw new Error("the config is locked");
        },
    removeWorkspace: (id) => {
      log.push(`remove:${id}`);
      if (options.removeFails) throw new Error("busy");
    },
    sendFirstMessage: async (paneId, text) => {
      log.push(`message:${paneId}`);
      if (options.sendFails) throw new Error("the queue is not reachable");
      messages.push(text);
    },
    openWorkspace: (id) => log.push(`open:${id}`),
    showWorkspace: (id) => log.push(`show:${id}`),
  };
  return { deps, log, consents, messages };
}

const byAgent = (repo: string, extra: Partial<FromGitRequest> = {}): FromGitRequest => ({
  repo,
  origin: { kind: "agent", pane: "Ava" },
  askerCwd: "C:\\dev\\hangar",
  ...extra,
});

const byPerson = (repo: string): FromGitRequest => ({
  repo,
  origin: { kind: "person" },
  askerCwd: "C:\\dev\\hangar",
});

const cloned = (log: string[]) => log.some((entry) => entry.startsWith("clone:"));

describe("the flow, in order", () => {
  it("goes parse, destination, ownership, clone, workspace, message, then opens it", async () => {
    const { deps, log, messages } = fakes({ mine: true });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(log).toEqual([
      "parse:acme/shop",
      "find:C:\\dev\\shop",
      "ownership:acme@github.com",
      "clone:https://github.com/acme/shop.git:C:\\dev:shop",
      "trust:C:\\dev\\shop",
      "workspace:shop:C:\\dev\\shop:behind",
      "message:p-orch",
      "open:w-new",
    ]);
    expect(outcome).toMatchObject({
      status: "created",
      mine: true,
      consented: false,
      workspace: { id: "w-new", name: "shop", cwd: "C:\\dev\\shop", orchestrator: { name: "Kai" } },
    });
    expect(messages).toHaveLength(1);
  });

  it("names the workspace as asked, and the folder after the repository all the same", async () => {
    const { deps, log } = fakes({ mine: true });
    const outcome = await workspaceFromGit(byAgent("acme/shop", { name: "  The shop " }), deps);
    expect(log).toContain("workspace:The shop:C:\\dev\\shop:behind");
    expect(outcome).toMatchObject({ status: "created", workspace: { name: "The shop" } });
  });

  it("refuses a name that is not one short line, before anything is cloned", async () => {
    for (const name of ["a\nb", "x".repeat(81)]) {
      const { deps, log } = fakes({ mine: true });
      const outcome = await workspaceFromGit(byAgent("acme/shop", { name }), deps);
      expect(outcome).toMatchObject({ status: "refused", kind: "invalidName" });
      expect(cloned(log)).toBe(false);
    }
  });
});

describe("whose repository it is", () => {
  it("does not ask the person about their own repository", async () => {
    const { deps, log, consents } = fakes({ mine: true });
    const outcome = await workspaceFromGit(byAgent("me/tool"), deps);
    expect(outcome.status).toBe("created");
    expect(log).not.toContain("consent");
    expect(consents).toEqual([]);
  });

  it("asks about one that is not theirs, with everything the card shows", async () => {
    const { deps, consents } = fakes({ mine: false });
    const outcome = await workspaceFromGit(byAgent("evil/tool"), deps);
    expect(consents).toEqual([
      {
        id: "consent-1",
        url: "https://github.com/evil/tool.git",
        host: "github.com",
        owner: "evil",
        name: "tool",
        destination: "C:\\dev\\tool",
        reason: "it belongs to someone else",
        by: "Ava",
      },
    ]);
    expect(outcome).toMatchObject({ status: "created", mine: false, consented: true });
  });

  it("asks when it cannot tell: not knowing is not owning", async () => {
    const { deps, consents } = fakes({ ownershipFails: true });
    await workspaceFromGit(byAgent("who/knows"), deps);
    expect(consents).toHaveLength(1);
    expect(consents[0].reason).toContain("gh is not signed in");
  });

  it("clones nothing, makes nothing, when the person refuses", async () => {
    const { deps, log } = fakes({ answer: "refuse" });
    const outcome = await workspaceFromGit(byAgent("evil/tool"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "declined" });
    expect(cloned(log)).toBe(false);
    expect(log.some((entry) => /^(workspace|message|open|discard|remove)/.test(entry))).toBe(false);
  });

  it("clones nothing, makes nothing, when nobody decided, and says nobody did", async () => {
    const { deps, log } = fakes({ answer: "timeout" });
    const outcome = await workspaceFromGit(byAgent("evil/tool"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "noDecision" });
    if (outcome.status === "refused") expect(outcome.reason).toMatch(/Nobody decided/);
    expect(cloned(log)).toBe(false);
    expect(log.some((entry) => /^(workspace|message|open|discard|remove)/.test(entry))).toBe(false);
  });

  it("skips the consent when the person clicked in Hangar, and brings the workspace forward", async () => {
    const { deps, log, consents } = fakes({ mine: false });
    const outcome = await workspaceFromGit(byPerson("evil/tool"), deps);
    expect(consents).toEqual([]);
    expect(log).toContain("workspace:tool:C:\\dev\\tool:front");
    expect(outcome).toMatchObject({ status: "created", mine: false, consented: false });
  });
});

describe("the trust question of Claude Code", () => {
  const trusted = (log: string[]) => log.filter((entry) => entry.startsWith("trust:"));

  it("trusts the clone of the person's own repository, before the workspace starts", async () => {
    const { deps, log } = fakes({ mine: true });
    const outcome = await workspaceFromGit(byAgent("me/tool"), deps);
    expect(trusted(log)).toEqual(["trust:C:\\dev\\tool"]);
    expect(log.findIndex((e) => e.startsWith("trust:"))).toBeLessThan(
      log.findIndex((e) => e.startsWith("workspace:")),
    );
    expect(outcome.status === "created" && outcome.notes.join(" ")).toMatch(/without the trust question/);
  });

  it("never trusts a repository that is not the person's, and says the person must answer", async () => {
    for (const make of [() => fakes({ mine: false }), () => fakes({ ownershipFails: true })]) {
      const { deps, log } = make();
      const outcome = await workspaceFromGit(byAgent("evil/tool"), deps);
      expect(trusted(log)).toEqual([]);
      expect(outcome.status === "created" && outcome.notes.join(" ")).toMatch(/not the person's own.*only they can answer/);
    }
  });

  it("does not trust anything when the person asked and the repository is foreign", async () => {
    const { deps, log } = fakes({ mine: false });
    await workspaceFromGit(byPerson("evil/tool"), deps);
    expect(trusted(log)).toEqual([]);
  });

  it("does not trust a folder that was never cloned", async () => {
    const { deps, log } = fakes({ mine: true, cloneThrows: true });
    await workspaceFromGit(byAgent("me/tool"), deps);
    expect(trusted(log)).toEqual([]);
  });

  it("carries on, and says the question may come, when the trust cannot be recorded", async () => {
    for (const options of [{ trustFails: true }, { noTrust: true }]) {
      const { deps, messages } = fakes({ mine: true, ...options });
      const outcome = await workspaceFromGit(byAgent("me/tool"), deps);
      expect(outcome.status).toBe("created");
      expect(messages).toHaveLength(1);
      expect(outcome.status === "created" && outcome.notes.join(" ")).toMatch(/may ask the person to trust/);
    }
  });
});

describe("what never reaches the clone", () => {
  it("stops a form Rust refuses at the parse", async () => {
    for (const repo of ["", "   ", "not a repo", "ext::sh -c touch /tmp/x", "--upload-pack=x/y"]) {
      const { deps, log } = fakes({ mine: true });
      const outcome = await workspaceFromGit(byAgent(repo), deps);
      expect(outcome, repo).toMatchObject({ status: "refused", kind: "invalidRepo" });
      expect(cloned(log), repo).toBe(false);
      expect(log.some((entry) => entry.startsWith("ownership") || entry === "consent"), repo).toBe(false);
    }
  });

  it("stops a parse that breaks", async () => {
    const { deps, log } = fakes({ mine: true });
    deps.parse = async () => {
      throw new Error("ipc is gone");
    };
    expect(await workspaceFromGit(byAgent("a/b"), deps)).toMatchObject({ status: "refused", kind: "invalidRepo" });
    expect(cloned(log)).toBe(false);
  });

  it("has nowhere to clone without a projects folder or a parent folder", async () => {
    const { deps, log } = fakes({ mine: true });
    for (const askerCwd of [null, "C:\\", "/"]) {
      const outcome = await workspaceFromGit(byAgent("a/b", { askerCwd }), deps);
      expect(outcome, String(askerCwd)).toMatchObject({ status: "refused", kind: "noDestination" });
    }
    expect(cloned(log)).toBe(false);
  });
});

describe("where it is cloned", () => {
  it("goes to the projects folder when one is set", async () => {
    const { deps, log } = fakes({ mine: true, projectsFolder: "D:\\Projects" });
    await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(log).toContain("clone:https://github.com/acme/shop.git:D:\\Projects:shop");
    expect(log).toContain("find:D:\\Projects\\shop");
  });

  it("goes beside the asking workspace otherwise, in the asker's own spelling of paths", async () => {
    const { deps, log } = fakes({ mine: true });
    await workspaceFromGit(byAgent("acme/shop", { askerCwd: "/home/me/code/hangar/" }), deps);
    expect(log).toContain("clone:https://github.com/acme/shop.git:/home/me/code:shop");
  });

  it("reads the projects folder from the settings when they have one, and only then", () => {
    expect(projectsFolderOf({})).toBeNull();
    expect(projectsFolderOf({ projectsFolder: "  " })).toBeNull();
    expect(projectsFolderOf({ projectsFolder: 3 })).toBeNull();
    expect(projectsFolderOf({ projectsFolder: " D:\\Projects " })).toBe("D:\\Projects");
  });

  it("does path arithmetic that works for both separators", () => {
    expect(parentFolder("C:\\dev\\hangar")).toBe("C:\\dev");
    expect(parentFolder("C:\\dev\\hangar\\")).toBe("C:\\dev");
    expect(parentFolder("C:\\dev")).toBe("C:\\");
    expect(parentFolder("C:\\")).toBeNull();
    expect(parentFolder("/home/me/x")).toBe("/home/me");
    expect(parentFolder("/x")).toBeNull();
    expect(joinFolder("C:\\dev", "shop")).toBe("C:\\dev\\shop");
    expect(joinFolder("C:\\", "shop")).toBe("C:\\shop");
    expect(joinFolder("/home/me/", "shop")).toBe("/home/me/shop");
    expect(sameFolder("C:\\Dev\\Shop\\", "c:/dev/shop")).toBe(true);
    expect(sameFolder("C:\\dev\\shop", "C:\\dev\\shop2")).toBe(false);
  });
});

describe("a workspace that is already there", () => {
  const existing: WorkspaceRef = { id: "w9", name: "Shop", cwd: "C:\\dev\\shop", orchestrator: { id: "p9", name: "Zed" } };

  it("is shown, not duplicated: nothing asked, cloned or made", async () => {
    const { deps, log, consents } = fakes({ existing, mine: false });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome).toMatchObject({ status: "existing", workspace: { id: "w9" } });
    expect(consents).toEqual([]);
    expect(log.filter((entry) => !entry.startsWith("parse") && !entry.startsWith("find"))).toEqual([]);
  });

  it("is brought forward when the person is the one asking", async () => {
    const { deps, log } = fakes({ existing });
    await workspaceFromGit(byPerson("acme/shop"), deps);
    expect(log).toContain("show:w9");
    const agent = fakes({ existing });
    await workspaceFromGit(byAgent("acme/shop"), agent.deps);
    expect(agent.log).not.toContain("show:w9");
  });

  it("says when it has no orchestrator", async () => {
    const { deps } = fakes({ existing: { ...existing, orchestrator: null } });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome.status === "existing" && outcome.notes.join(" ")).toMatch(/no orchestrator/);
  });

  it("is seen again after the wait for the person, as one may have opened it meanwhile", async () => {
    const { deps, log } = fakes({ mine: false });
    let found: WorkspaceRef | null = null;
    deps.findWorkspace = () => found;
    deps.consent = async () => {
      found = existing;
      return "allow";
    };
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome.status).toBe("existing");
    expect(cloned(log)).toBe(false);
  });

  it("makes two requests for one folder at once into one", async () => {
    const { deps, log } = fakes({ mine: true });
    const [first, second] = await Promise.all([
      workspaceFromGit(byAgent("acme/shop"), deps),
      workspaceFromGit(byAgent("acme/shop"), deps),
    ]);
    expect(first).toBe(second);
    expect(log.filter((entry) => entry.startsWith("clone:"))).toHaveLength(1);
  });

  it("does not hand the first repository's result to another repository for the same folder", async () => {
    const { deps, log } = fakes({ mine: true });
    let release: () => void = () => undefined;
    const gate = new Promise<void>((resolve) => (release = resolve));
    const slowClone = deps.clone;
    deps.clone = async (...args) => {
      await gate;
      return slowClone(...args);
    };
    const good = workspaceFromGit(byAgent("good/theirs"), deps);
    await vi.waitFor(() => expect(log.some((entry) => entry.startsWith("ownership:"))).toBe(true));
    const evil = await workspaceFromGit(byAgent("evil/theirs"), deps);
    expect(evil).toMatchObject({ status: "refused", kind: "folderBusy" });
    if (evil.status === "refused") expect(evil.reason).toContain("evil/theirs");
    release();
    expect(await good).toMatchObject({ status: "created" });
    const clones = log.filter((entry) => entry.startsWith("clone:"));
    expect(clones).toHaveLength(1);
    expect(clones[0]).toContain("good/theirs");
    // Once the first is over, the folder is free for a request of its own.
    expect(await workspaceFromGit(byAgent("evil/theirs"), fakes({ mine: true }).deps)).toMatchObject({
      status: "created",
    });
  });

  it("refuses at once, through the real queue, a foreign repository for a folder already waiting", async () => {
    const queue = createConsentQueue();
    const first = fakes();
    first.deps.consent = (asked) => queue.ask(asked);
    const waiting = workspaceFromGit(byAgent("good/theirs"), first.deps);
    await vi.waitFor(() => expect(queue.pending()).toHaveLength(1));
    // The flow-level guard catches it before the queue: nothing is asked, nothing cloned.
    const second = fakes();
    second.deps.consent = (asked) => queue.ask(asked);
    expect(await workspaceFromGit(byAgent("evil/theirs"), second.deps)).toMatchObject({
      status: "refused",
      kind: "folderBusy",
    });
    expect(queue.pending()).toHaveLength(1);
    expect(cloned(second.log)).toBe(false);
    queue.answer("consent-1", true);
    expect((await waiting).status).toBe("created");
    expect(first.log.filter((entry) => entry.startsWith("clone:"))).toHaveLength(1);
  });

  it("reads a busy answer from the consent as a refusal with its reason", async () => {
    const { deps, log } = fakes({ answer: "busy" });
    const outcome = await workspaceFromGit(byAgent("evil/theirs"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "folderBusy" });
    expect(cloned(log)).toBe(false);
  });
});

describe("a step that fails leaves nothing half made", () => {
  it("reports a clone Rust refused, with nothing to take back", async () => {
    const { deps, log } = fakes({
      mine: true,
      discard: true,
      clone: { status: "refused", kind: "exists", reason: "shop already exists and is not this repository" },
    });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "cloneFailed" });
    if (outcome.status === "refused") expect(outcome.reason).toContain("already exists");
    expect(log.some((entry) => /^(workspace|discard|remove)/.test(entry))).toBe(false);
  });

  it("reports a broken IPC as a failed clone", async () => {
    const { deps, log } = fakes({ mine: true, cloneThrows: true, discard: true });
    expect(await workspaceFromGit(byAgent("acme/shop"), deps)).toMatchObject({ status: "refused", kind: "cloneFailed" });
    expect(log.some((entry) => /^(workspace|discard|remove)/.test(entry))).toBe(false);
  });

  it("takes back the workspace and the folder it cloned when the workspace cannot be made", async () => {
    const { deps, log } = fakes({ mine: true, discard: true, createFails: true });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "failed" });
    if (outcome.status === "refused") expect(outcome.reason).toMatch(/Nothing was left behind/);
    // The workspace never existed, the folder did.
    expect(log).toContain("discard:C:\\dev\\shop");
    expect(log.some((entry) => entry.startsWith("remove:"))).toBe(false);
    expect(log).not.toContain("open:w-new");
  });

  it("takes back both when the first message cannot be queued, before anything was started", async () => {
    const { deps, log } = fakes({ mine: true, discard: true, sendFails: true });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "failed" });
    expect(log).toContain("remove:w-new");
    expect(log).toContain("discard:C:\\dev\\shop");
    expect(log).not.toContain("open:w-new");
    expect(log.indexOf("remove:w-new")).toBeLessThan(log.indexOf("discard:C:\\dev\\shop"));
  });

  it("removes the folder only when this call made it", async () => {
    const { deps, log } = fakes({
      mine: true,
      discard: true,
      sendFails: true,
      clone: { status: "cloned", path: "C:\\dev\\shop", reused: true, branch: "main" },
    });
    const outcome = await workspaceFromGit(byAgent("acme/shop"), deps);
    expect(outcome).toMatchObject({ status: "refused", kind: "failed" });
    expect(log).toContain("remove:w-new");
    expect(log.some((entry) => entry.startsWith("discard:"))).toBe(false);
  });

  it("says what it could not take back instead of claiming a clean start", async () => {
    const withoutDiscard = fakes({ mine: true, sendFails: true });
    const a = await workspaceFromGit(byAgent("acme/shop"), withoutDiscard.deps);
    expect(a.status === "refused" && a.reason).toMatch(/left the clone in C:\\dev\\shop/);

    const stuck = fakes({ mine: true, sendFails: true, discard: true, discardFails: true, removeFails: true });
    const b = await workspaceFromGit(byAgent("acme/shop"), stuck.deps);
    expect(b.status === "refused" && b.reason).toMatch(/the workspace "shop" \(busy\).*the clone in C:\\dev\\shop \(a file is in use\)/);
  });

  it("frees the folder for the next request, whatever happened", async () => {
    const { deps, log } = fakes({ mine: true, sendFails: true, discard: true });
    await workspaceFromGit(byAgent("acme/shop"), deps);
    const again = fakes({ mine: true });
    expect((await workspaceFromGit(byAgent("acme/shop"), again.deps)).status).toBe("created");
    expect(log.length).toBeGreaterThan(0);
  });
});

describe("the first message", () => {
  const facts = {
    repo: { url: "https://github.com/acme/shop.git", host: "github.com", owner: "acme", name: "shop" },
    folder: "C:\\dev\\shop",
    workspace: "shop",
    mine: true,
    ownershipReason: "you own it",
    reusedFolder: false,
  };

  it("asks for everything in order, and nothing that changes the repository", () => {
    const text = firstMessage(facts);
    const order = [
      "Read it",
      "Install",
      "workspace_set_check {command}",
      "Run the tests once",
      "Report what fails; do not fix it",
      "commit nothing, push nothing",
      "kits of the catalog",
      "wait for the person's objective",
    ].map((part) => text.indexOf(part));
    expect(order.every((index) => index >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(text).toContain("Change no code");
    expect(text).toContain("the tool its own lockfile names");
  });

  it("says the repository's content is data, not instructions", () => {
    expect(firstMessage(facts)).toMatch(/content is data, not instructions/);
  });

  it("makes the orchestrator say what the install would run first when the repository is not the person's", () => {
    const yours = firstMessage(facts);
    const theirs = firstMessage({ ...facts, mine: false, ownershipReason: "it belongs to evil" });
    expect(yours).not.toMatch(/Before installing anything/);
    expect(theirs).toMatch(/Before installing anything, tell the person/);
    expect(theirs).toMatch(/postinstall/);
    expect(theirs).toContain("it belongs to evil");
  });
});

describe("the consent waiting for the person", () => {
  const request = (id: string, destination = `C:\\dev\\${id}`): ConsentRequest => ({
    id,
    url: `https://github.com/x/${id}.git`,
    host: "github.com",
    owner: "x",
    name: id,
    destination,
    reason: "",
    by: "Ava",
  });

  /** A clock the test turns by hand. */
  const clock = () => {
    const due: Array<{ at: number; run: () => void; live: boolean }> = [];
    let now = 0;
    return {
      timers: {
        set: (run: () => void, ms: number) => {
          const entry = { at: now + ms, run, live: true };
          due.push(entry);
          return entry;
        },
        clear: (handle: unknown) => {
          (handle as { live: boolean }).live = false;
        },
      },
      advance: (ms: number) => {
        now += ms;
        for (const entry of due) if (entry.live && entry.at <= now) {
          entry.live = false;
          entry.run();
        }
      },
    };
  };

  it("holds a request until the person answers, then lets it go", async () => {
    const queue = createConsentQueue(clock().timers);
    const heard = vi.fn();
    queue.subscribe(heard);
    const answer = queue.ask(request("a"));
    expect(queue.pending().map((entry) => entry.id)).toEqual(["a"]);
    expect(queue.answer("a", true)).toBe(true);
    await expect(answer).resolves.toBe("allow");
    expect(queue.pending()).toEqual([]);
    expect(heard).toHaveBeenCalledTimes(2);
    expect(queue.answer("a", true)).toBe(false);
  });

  it("answers refuse when the person refuses", async () => {
    const queue = createConsentQueue(clock().timers);
    const answer = queue.ask(request("a"));
    queue.answer("a", false);
    await expect(answer).resolves.toBe("refuse");
  });

  it("gives up after a few minutes: nobody decided", async () => {
    const { timers, advance } = clock();
    const queue = createConsentQueue(timers);
    const answer = queue.ask(request("a"));
    advance(CONSENT_TIMEOUT_MS - 1);
    expect(queue.pending()).toHaveLength(1);
    advance(1);
    await expect(answer).resolves.toBe("timeout");
    expect(queue.pending()).toEqual([]);
    expect(CONSENT_TIMEOUT_MS).toBeGreaterThanOrEqual(2 * 60_000);
    expect(CONSENT_TIMEOUT_MS).toBeLessThanOrEqual(10 * 60_000);
  });

  it("lets a late click fall on nothing", async () => {
    const { timers, advance } = clock();
    const queue = createConsentQueue(timers);
    const answer = queue.ask(request("a"));
    advance(CONSENT_TIMEOUT_MS);
    await answer;
    expect(queue.answer("a", true)).toBe(false);
  });

  it("shares one question between two requests for one repository in one folder", async () => {
    const queue = createConsentQueue(clock().timers);
    const first = queue.ask(request("a", "C:\\dev\\same"));
    const second = queue.ask({ ...request("b", "c:/dev/same"), url: "https://github.com/x/a" });
    expect(queue.pending()).toHaveLength(1);
    queue.answer("a", true);
    await expect(Promise.all([first, second])).resolves.toEqual(["allow", "allow"]);
  });

  it("never lets another repository ride on the answer to a card it never showed", async () => {
    const queue = createConsentQueue(clock().timers);
    const good = queue.ask({ ...request("a", "C:\\dev\\theirs"), url: "https://github.com/good/theirs.git" });
    const evil = queue.ask({ ...request("b", "C:\\dev\\theirs"), url: "https://github.com/evil/theirs.git" });
    // Refused at once, with no second card.
    await expect(evil).resolves.toBe("busy");
    expect(queue.pending().map((entry) => entry.url)).toEqual(["https://github.com/good/theirs.git"]);
    queue.answer("a", true);
    await expect(good).resolves.toBe("allow");
  });

  it("tells the trace once per question, shared askers included", async () => {
    const answered = vi.fn();
    const { timers, advance } = clock();
    const queue = createConsentQueue(timers, answered);
    void queue.ask(request("a"));
    void queue.ask(request("a"));
    void queue.ask({ ...request("b", "C:\\dev\\a"), url: "https://github.com/other/a.git" });
    expect(answered).not.toHaveBeenCalled();
    queue.answer("a", false);
    expect(answered).toHaveBeenCalledTimes(1);
    expect(answered).toHaveBeenLastCalledWith(expect.objectContaining({ id: "a" }), "refuse");
    void queue.ask(request("c"));
    advance(CONSENT_TIMEOUT_MS);
    expect(answered).toHaveBeenLastCalledWith(expect.objectContaining({ id: "c" }), "timeout");
    expect(answered).toHaveBeenCalledTimes(2);
  });

  it("goes on when the trace throws", async () => {
    const queue = createConsentQueue(clock().timers, () => {
      throw new Error("disk full");
    });
    const answer = queue.ask(request("a"));
    queue.answer("a", true);
    await expect(answer).resolves.toBe("allow");
  });

  it("keeps the order of different questions, and a stable list between changes", () => {
    const queue = createConsentQueue(clock().timers);
    void queue.ask(request("a"));
    const before = queue.pending();
    void queue.ask(request("b"));
    expect(queue.pending().map((entry) => entry.id)).toEqual(["a", "b"]);
    expect(queue.pending()).toBe(queue.pending());
    expect(queue.pending()).not.toBe(before);
  });

  it("waits through the flow: a click on Clone lets the clone start, none lets it run out", async () => {
    const { timers, advance } = clock();
    const queue = createConsentQueue(timers);
    const { deps, log } = fakes();
    deps.consent = (asked) => queue.ask(asked);
    const waiting = workspaceFromGit(byAgent("evil/tool"), deps);
    await vi.waitFor(() => expect(queue.pending()).toHaveLength(1));
    expect(cloned(log)).toBe(false);
    advance(CONSENT_TIMEOUT_MS);
    expect(await waiting).toMatchObject({ status: "refused", kind: "noDecision" });
    expect(cloned(log)).toBe(false);

    const clicked = fakes();
    clicked.deps.consent = (asked) => queue.ask(asked);
    const again = workspaceFromGit(byAgent("evil/tool"), clicked.deps);
    await vi.waitFor(() => expect(queue.pending()).toHaveLength(1));
    queue.answer("consent-1", true);
    expect((await again).status).toBe("created");
  });
});

describe("the trace of each consent answer", () => {
  const asked = (destination: string): ConsentRequest => ({
    id: "c1",
    url: "https://github.com/evil/theirs.git",
    host: "github.com",
    owner: "evil",
    name: "theirs",
    destination,
    reason: "",
    by: "Ava",
  });

  it("names the repository, the folder's name and how it ended, never the path", () => {
    const request = asked("C:\\Users\\me\\Documents\\projects\\theirs");
    const lines = (["allow", "refuse", "timeout"] as const).map((answer) => consentTrace(request, answer, "en", 5));
    expect(lines.map((line) => line.text)).toEqual([
      'Clone of https://github.com/evil/theirs.git into the folder "theirs": you clicked Clone',
      'Clone of https://github.com/evil/theirs.git into the folder "theirs": you clicked Refuse',
      'Clone of https://github.com/evil/theirs.git into the folder "theirs": no answer in time, nothing cloned',
    ]);
    for (const line of lines) {
      expect(line).toMatchObject({ at: 5, workspace: "", kind: "gitConsent" });
      expect(line.text).not.toContain("Users");
      expect(line.text).not.toContain("Documents");
    }
    expect(consentTrace(request, "allow", "fr", 5).text).toContain("tu as cliqué sur Cloner");
  });

  it("keeps the last part of a folder, either separator, trailing one included", () => {
    expect(folderName("/home/me/code/theirs/")).toBe("theirs");
    expect(folderName("C:\\a\\b")).toBe("b");
  });

  it("writes one line to the activity log per answer, and none for a refused-at-once request", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const call = vi.mocked(invoke);
    call.mockReset();
    call.mockResolvedValue(undefined);
    recordConsent(asked("C:\\x\\theirs"), "allow");
    recordConsent(asked("C:\\x\\theirs"), "busy");
    expect(call).toHaveBeenCalledTimes(1);
    expect(call).toHaveBeenCalledWith(
      "activity_append",
      expect.objectContaining({ entries: [expect.objectContaining({ kind: "gitConsent" })] }),
    );
    call.mockRejectedValue(new Error("disk"));
    expect(() => recordConsent(asked("C:\\x\\theirs"), "refuse")).not.toThrow();
  });
});

describe("the form behind « From a Git repository »", () => {
  const ok: RepoParse = {
    status: "ok",
    url: "https://github.com/acme/shop.git",
    host: "github.com",
    owner: "acme",
    name: "shop",
  };
  const refused: RepoParse = { status: "refused", reason: "the address is not valid" };
  const form = (patch: Partial<FromGitFormState> = {}): FromGitFormState => ({
    input: "acme/shop",
    parse: ok,
    parent: "D:\\Projects",
    running: false,
    ...patch,
  });

  it("clones into the typed folder, else the setting, else next to the current workspace", () => {
    expect(resolveParent(" E:\\Repos ", "D:\\Projects", "C:\\Work\\app")).toBe("E:\\Repos");
    expect(resolveParent("", "D:\\Projects", "C:\\Work\\app")).toBe("D:\\Projects");
    expect(resolveParent(null, "  ", "C:\\Work\\app")).toBe("C:\\Work");
    expect(resolveParent(undefined, null, "/home/me/code/app/")).toBe("/home/me/code");
    expect(resolveParent(null, null, null)).toBeNull();
    expect(resolveParent(null, null, "C:\\")).toBeNull();
  });

  it("names the destination from the folder and the repository", () => {
    expect(cloneDestination("D:\\Projects", "shop")).toBe("D:\\Projects\\shop");
    expect(cloneDestination("/home/me/code/", "shop")).toBe("/home/me/code/shop");
    expect(cloneDestination("  ", "shop")).toBeNull();
    expect(cloneDestination("D:\\Projects", null)).toBeNull();
  });

  it("reads the form's status", () => {
    expect(fromGitStatus(form({ input: "  \n" }))).toBe("empty");
    expect(fromGitStatus(form({ parse: null }))).toBe("checking");
    expect(fromGitStatus(form({ parse: refused }))).toBe("refused");
    expect(fromGitStatus(form({ parent: null }))).toBe("noFolder");
    expect(fromGitStatus(form({ parent: "  " }))).toBe("noFolder");
    expect(fromGitStatus(form())).toBe("ready");
    expect(fromGitStatus(form({ running: true, input: "" }))).toBe("running");
  });

  it("says what the button says, and enables it only when ready", () => {
    expect(fromGitButton("ready")).toEqual({ label: "create.gitSubmit", disabled: false });
    expect(fromGitButton("running")).toEqual({ label: "create.gitRunning", disabled: true });
    for (const status of ["empty", "checking", "refused", "noFolder"] as const) {
      expect(fromGitButton(status)).toEqual({ label: "create.gitSubmit", disabled: true });
    }
    expect(fromGitFieldsDisabled("running")).toBe(true);
    expect(fromGitFieldsDisabled("ready")).toBe(false);
  });

  it("Enter starts a ready form, Escape closes unless something runs", () => {
    expect(fromGitKey("Enter", "ready")).toBe("start");
    for (const status of ["empty", "checking", "refused", "noFolder", "running"] as const) {
      expect(fromGitKey("Enter", status)).toBeNull();
    }
    expect(fromGitKey("Escape", "ready")).toBe("close");
    expect(fromGitKey("Escape", "refused")).toBe("close");
    expect(fromGitKey("Escape", "running")).toBeNull();
    expect(fromGitKey("a", "ready")).toBeNull();
  });

  it("keeps a failure to one short line", () => {
    expect(oneLine("The clone failed: repository not found")).toBe("The clone failed: repository not found");
    expect(oneLine("\n  first   line \nsecond")).toBe("first line");
    expect(oneLine("")).toBe("");
    const cut = oneLine("x".repeat(400));
    expect(cut).toHaveLength(160);
    expect(cut.endsWith("…")).toBe(true);
  });
});
