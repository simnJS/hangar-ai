"use strict";
// One PreToolUse event, from its text to the hook's answer: nothing, or the deny.
// A one-time pass lets the call go on to Claude Code's own permission flow: the
// guard refuses or says nothing, and never answers "allow".

const crypto = require("node:crypto");
const { createContext, dataFolder } = require("./context.js");
const { check, filesOfCall } = require("./rules.js");
const store = require("./store.js");

const COMMAND_TOOLS = new Set(["Bash", "PowerShell", "Monitor"]);
const FILE_TOOLS = new Set(["Read", "Edit", "Write", "NotebookEdit", "LSP"]);
const SENDING_TOOLS = new Set(["Artifact", "ArtifactData", "DesignSync"]);
const TOOLS = new Set([...COMMAND_TOOLS, ...FILE_TOOLS, ...SENDING_TOOLS, "Grep"]);

/** Who is asking. A subagent is not the agent that started it. */
function whoIs(event, env) {
  const base = env.HANGAR_PANE_ID || event.session_id || "unknown";
  const name = env.HANGAR_PANE_NAME || (env.HANGAR_PANE_ID ? env.HANGAR_PANE_ID.slice(0, 8) : String(event.session_id || "agent").slice(0, 8));
  return {
    id: event.agent_id ? `${base}/${event.agent_id}` : base,
    name: event.agent_type ? `${name} · ${event.agent_type}` : name,
  };
}

/** The exact thing a pass is for, and how to show it to the person. */
function subjectOf(event) {
  const input = event.tool_input || {};
  if (COMMAND_TOOLS.has(event.tool_name)) return { subject: input.command, shown: `${event.tool_name === "Bash" ? "" : `${event.tool_name}: `}${input.command}` };
  if (event.tool_name === "Grep") {
    const where = input.path || input.glob || ".";
    return { subject: JSON.stringify([input.pattern, input.path, input.glob]), shown: `Grep ${input.pattern} in ${where}` };
  }
  const files = filesOfCall(event.tool_name, input);
  return { subject: files.join("\n"), shown: `${event.tool_name} ${files.join(", ")}` };
}

function denyAnswer(why) {
  return {
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: "deny",
      permissionDecisionReason:
        `The guard refused this: ${why}. Do not look for another way to do it. ` +
        "Tell the person what you were trying to do and why you need this. " +
        "They can let this exact command through once, for 2 minutes, from the Guardrails (Garde-fous) tab; " +
        "retry the same command, unchanged and with the same tool, only after they tell you to.",
    },
  };
}

function logError(dir, error, now) {
  try {
    store.appendError(dir, (error && error.message) || error, now);
  } catch {
    /* nowhere to say it */
  }
}

/**
 * @param {string} raw        what Claude Code wrote on stdin
 * @param {object} [options]  { env, now, openTab(), context }: `context` is createContext's options
 * @returns {object|null}     the JSON to print, or null to let the call through
 */
function decide(raw, options = {}) {
  const env = options.env || process.env;
  const now = options.now || Date.now();
  const openTab = options.openTab || (() => undefined);
  const dir = (options.context && options.context.dataDir) || dataFolder(env);
  try {
    const event = JSON.parse(raw);
    if (!event || !TOOLS.has(event.tool_name)) return null;
    const ctx = createContext(event, {
      env,
      ...options.context,
      report: (message) => logError(dir, new Error(message), now),
    });
    const refused = check(event, ctx);
    if (!refused) return null;

    const agent = whoIs(event, env);
    const { subject, shown } = subjectOf(event);
    const key = store.passKey(agent.id, event.tool_name, subject);
    try {
      if (store.takePass(dir, key, agent.id, now)) {
        store.appendEvent(dir, { type: "pass-used", at: now, key, agent: agent.name });
        return null;
      }
    } catch (error) {
      logError(dir, error, now);
    }

    // A refusal stands whether or not it can be written down.
    try {
      store.appendEvent(dir, {
        type: "refusal",
        at: now,
        key,
        agent: agent.name,
        agentId: agent.id,
        tool: event.tool_name,
        command: String(shown).slice(0, store.COMMAND_MAX),
        length: String(shown).length,
        rule: refused.rule,
        kind: refused.kind,
        subject: refused.subject,
        why: refused.why,
        folder: ctx.cwd,
      });
      store.sweepPasses(dir, now);
    } catch (error) {
      logError(dir, error, now);
    }
    try {
      openTab();
    } catch (error) {
      logError(dir, error, now);
    }
    return denyAnswer(refused.why);
  } catch (error) {
    // The guard's own failure is never the agent's: the call goes through, and it says so.
    logError(dir, error, now);
    return null;
  }
}

module.exports = { decide, whoIs, denyAnswer };
