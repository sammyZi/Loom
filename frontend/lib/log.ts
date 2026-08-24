import { type AgentEvent } from "./api";

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
    default:
      return { text: name, detail: "" };
  }
}
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
