import { type AgentEvent } from "./api";

export type LogItem = { kind: string; text: string };
export type ToolGroup = { kind: "tools"; items: string[] };
export type Group = LogItem | ToolGroup;

export function formatEvent(ev: AgentEvent): LogItem | null {
  switch (ev.type) {
    case "token":
      return { kind: "token", text: ev.text };
    case "think":
      return { kind: "think", text: ev.text };
    case "tool_call":
      return { kind: "tool", text: ev.name };
    case "error":
      return { kind: "err", text: ev.message };
    default:
      return null;
  }
}

/** Streamed text arrives in fragments; append it to the run in progress. */
export function mergeLog(prev: LogItem[], line: LogItem): LogItem[] {
  const last = prev[prev.length - 1];
  if ((line.kind === "token" || line.kind === "think") && last?.kind === line.kind) {
    const copy = prev.slice();
    copy[copy.length - 1] = { kind: line.kind, text: last.text + line.text };
    return copy;
  }
  return [...prev, line];
}

/** Fold runs of tool calls into one collapsible line. */
export function groupLog(log: LogItem[]): Group[] {
  const out: Group[] = [];
  for (const l of log) {
    if (l.kind !== "tool") {
      out.push(l);
      continue;
    }
    const last = out[out.length - 1];
    if (last && "items" in last) last.items.push(l.text);
    else out.push({ kind: "tools", items: [l.text] });
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

export function mmss(sec: number) {
  return `${Math.floor(sec / 60)}m ${String(sec % 60).padStart(2, "0")}s`;
}

export function errText(e: unknown) {
  return e instanceof Error ? e.message : String(e);
}

export function baseName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() || path;
}
