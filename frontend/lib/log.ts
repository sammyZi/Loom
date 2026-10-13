// `import type` (not `import { type … }`): the type-only form is erased
// outright, which is what lets lib/log.test.mjs load this module directly
// under node's type stripping instead of resolving ./api at runtime.
import type { AgentEvent } from "./api";

/** `images` is data: URLs — small enough for the odd screenshot, and it has
 *  to survive round-tripping through the session's stored JSON log.
 *  `revert` is the snapshot for "undo this message" (path -> content before
 *  this run, `null` meaning the run created it); `reverted` marks it used. */
export type LogItem = {
  kind: string;
  text: string;
  detail?: string;
  images?: string[];
  revert?: Record<string, string | null>;
  reverted?: boolean;
  /** Lines this edit added / removed, for the +N −M chip on a tool group. */
  add?: number;
  del?: number;
};

/**
 * Turn a tool call into a "Read foo.ts" / "Edited bar.rs" / "Ran cargo test" line,
 * the way Cowork narrates what it is doing.
 */
export function toolLabel(name: string, input: unknown): { text: string; detail: string } {
  const o = (input ?? {}) as Record<string, unknown>;
  const path = typeof o.path === "string" ? o.path : "";
  const program = typeof o.program === "string" ? o.program : "";
  const args = Array.isArray(o.args) ? o.args.filter((a) => typeof a === "string").join(" ") : "";

  switch (name) {
    case "read_file":
      return { text: "Read", detail: baseName(path) };
    case "edit_file":
      return { text: "Edited", detail: baseName(path) };
    case "run_command":
      return { text: "Ran", detail: [program, args].filter(Boolean).join(" ") };
    case "check_code":
      return { text: "Checked code", detail: "" };
    case "run_tests":
      return { text: "Ran tests", detail: "" };
    case "write_file":
      return { text: "Wrote", detail: baseName(path) };
    case "list_files":
      return { text: "Listed", detail: path || "." };
    case "search_files":
      return { text: "Searched", detail: str(o.query) };
    case "web_search":
      return { text: "Searched the web", detail: str(o.query) };
    case "web_fetch":
      return { text: "Fetched", detail: str(o.url) };
    case "todo_write":
      return { text: "Updated the plan", detail: "" };
    case "task":
      return { text: `Asked ${str(o.agent) || "a"} agent`, detail: clip(str(o.prompt)) };
    case "spawn_agent":
      return { text: `Started ${str(o.agent) || "a"} agent`, detail: clip(str(o.prompt)) };
    case "wait_agent":
      return { text: "Waiting for agents", detail: "" };
    case "close_agent":
      return { text: "Stopped agent", detail: str(o.id) };
    case "skill":
      return { text: "Loaded skill", detail: str(o.name) };
    case "browser_open":
      return { text: "Opened", detail: str(o.url) };
    default:
      return { text: name, detail: "" };
  }
}
const str = (v: unknown) => (typeof v === "string" ? v : "");
const clip = (s: string) => (s.length > 80 ? `${s.slice(0, 80)}…` : s);

export type ToolGroup = { kind: "tools"; items: LogItem[] };
export type Group = LogItem | ToolGroup;

export function formatEvent(ev: AgentEvent): LogItem | null {
  switch (ev.type) {
    case "token":
      return { kind: "token", text: ev.text };
    case "think":
      return { kind: "think", text: ev.text };
    case "tool_call": {
      const { text, detail } = toolLabel(ev.name, ev.input);
      return { kind: "tool", text, detail };
    }
    case "ask":
      return {
        kind: "tool",
        text: "Wants to run",
        detail: [ev.program, ev.args].filter(Boolean).join(" "),
      };
    case "error":
      return { kind: "err", text: ev.message };
    default:
      return null;
  }
}

/** Streamed text arrives in fragments; append it to the run in progress. */
/**
 * The planner marks non-coding answers with a leading NO_CODE: so the orchestrator
 * can stop early. Tokens stream to the UI before that happens, and the marker can be
 * split across deltas, so it is stripped from the accumulated text rather than per chunk.
 */
// The colon is required: without it a half-arrived "NO_CODE" would be stripped
// while still streaming, and the colon that followed would be left behind.
const MARKER = /^\s*NO_CODE\s*:\s*/;

export function mergeLog(prev: LogItem[], line: LogItem): LogItem[] {
  const last = prev[prev.length - 1];
  if ((line.kind === "token" || line.kind === "think") && last?.kind === line.kind) {
    const copy = prev.slice();
    const joined = last.text + line.text;
    copy[copy.length - 1] = {
      kind: line.kind,
      text: line.kind === "token" ? joined.replace(MARKER, "") : joined,
    };
    return copy;
  }
  if (line.kind === "token" || line.kind === "ok") {
    return [...prev, { ...line, text: line.text.replace(MARKER, "") }];
  }
  return [...prev, line];
}

/** Fold runs of tool calls into one collapsible line. */
export function groupLog(log: LogItem[]): Group[] {
  const out: Group[] = [];
  // One reasoning block per turn. The model thinks, calls tools, thinks again —
  // which produced a separate "Thinking" row each time and read as the same
  // thing repeating. It is one continuous stream, so it gets one block, reset
  // when the user speaks again.
  let think: LogItem | null = null;
  for (const l of log) {
    // A turn that produced only tool calls leaves an empty text item behind.
    // Rendered, it is an invisible block with a copy button wedged between tool
    // groups, and it also stops adjacent groups from merging.
    if (l.kind !== "tool" && !l.text.trim()) continue;
    if (l.kind === "user") {
      think = null;
      out.push(l);
      continue;
    }
    if (l.kind === "think") {
      if (think) {
        // Copied on first sight, so appending never mutates the caller's log.
        think.text += `\n${l.text}`;
        continue;
      }
      think = { ...l };
      out.push(think);
      continue;
    }
    if (l.kind !== "tool") {
      out.push(l);
      continue;
    }
    const last = out[out.length - 1];
    if (last && "items" in last) last.items.push(l);
    else out.push({ kind: "tools", items: [l] });
  }
  return out;
}

/** Credit a file diff to the edit that produced it: the newest tool row. */
export function attachDiff(log: LogItem[], diff: string): LogItem[] {
  let i = log.length - 1;
  while (i >= 0 && log[i].kind !== "tool") i--;
  if (i < 0) return log;
  const { add, del } = countDiff(diff);
  const copy = log.slice();
  copy[i] = { ...copy[i], add: (copy[i].add ?? 0) + add, del: (copy[i].del ?? 0) + del };
  return copy;
}

/**
 * One line for a folded tool group, Claude Code style: "Read App.tsx, edited
 * 2 files, ran a command". It used to say "Ran N commands" for everything,
 * including groups that only read files.
 */
export function summarizeTools(items: LogItem[]): string {
  const files = (verbs: string[]) => items.filter((t) => verbs.includes(t.text));
  const reads = files(["Read"]);
  const edits = files(["Edited", "Wrote"]);
  const runs = files(["Ran"]).length;
  const other = items.length - reads.length - edits.length - runs;
  const names = (xs: LogItem[]) => [...new Set(xs.map((x) => x.detail || ""))];
  const what = (xs: LogItem[]) => {
    const n = names(xs);
    return n.length === 1 && n[0] ? n[0] : `${n.length} files`;
  };
  const parts: string[] = [];
  const r = names(reads);
  const e = names(edits);
  if (r.length === 1 && e.length === 1 && r[0] === e[0] && r[0]) {
    parts.push(`read and edited ${r[0]}`);
  } else {
    if (reads.length) parts.push(`read ${what(reads)}`);
    if (edits.length) parts.push(`edited ${what(edits)}`);
  }
  if (runs) parts.push(runs === 1 ? "ran a command" : `ran ${runs} commands`);
  if (other) parts.push(`used ${other} tool${other === 1 ? "" : "s"}`);
  const line = parts.join(", ");
  return line.charAt(0).toUpperCase() + line.slice(1);
}

export function countDiff(diff: string) {
  let add = 0;
  let del = 0;
  for (const l of diff.split("\n")) {
    if (l.startsWith("+") && !l.startsWith("+++")) add++;
    else if (l.startsWith("-") && !l.startsWith("---")) del++;
  }
  return { add, del };
}

/** Compact elapsed time: 15s, then 1m 03s once past a minute. */
export function secs(sec: number) {
  return sec < 60 ? `${sec}s` : `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, "0")}s`;
}

export function mmss(sec: number) {
  return `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, "0")}s`;
}

export function errText(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}

export function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}

/**
 * Ceiling on a terminal's scrollback, in characters. A dev server left running
 * for an afternoon streams tens of megabytes; every chunk rebuilt the string
 * and re-rendered it, so the panel got slower the longer it ran and the tab
 * eventually ran out of memory.
 */
export const LOG_CAP = 200_000;

/** Append to a live log, dropping the oldest output once past the cap. */
export function appendLog(prev: string, chunk: string): string {
  const next = prev + chunk;
  if (next.length <= LOG_CAP) return next;
  // Cut on a line boundary so the first surviving line is not half a line.
  const cut = next.length - LOG_CAP;
  const nl = next.indexOf("\n", cut);
  return `[… earlier output trimmed …]\n${next.slice(nl >= 0 ? nl + 1 : cut)}`;
}
