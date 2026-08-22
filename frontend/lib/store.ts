import { apiBase } from "./api";
import type { LogItem } from "./log";

/**
 * Sessions live in SQLite in the Rust backend (see cli/src/db.rs), not in the
 * browser. That is what makes history shared across ports, unbounded by the
 * ~5MB localStorage quota, and durable across a browser cache clear.
 */
export type Session = {
  id: string;
  folder: string;
  title: string;
  log: LogItem[];
  /** Last activity, epoch millis. */
  at: number;
  /** Start time. List order uses this so rows never re-shuffle. */
  created?: number;
  archived?: boolean;
};

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`${apiBase()}/sessions${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers || {}) },
  });
  if (!r.ok) throw new Error((await r.text()) || r.statusText);
  return (await r.json()) as T;
}

/** Every stored session across all projects, newest first. Archived are hidden. */
export async function loadAllSessions(): Promise<Session[]> {
  try {
    const { sessions } = await call<{ sessions: Session[] }>("");
    return sessions ?? [];
  } catch {
    return [];
  }
}

export async function saveSession(s: Session): Promise<void> {
  // `created` must come after the spread: `{ created: s.at, ...s }` let a stale
  // s.created override the intended start time, reshuffling the sidebar list.
  await call("", { method: "PUT", body: JSON.stringify({ ...s, created: s.created || s.at }) });
}

export async function deleteSession(id: string): Promise<void> {
  await call(`/${encodeURIComponent(id)}`, { method: "DELETE" });
}

export async function renameSession(id: string, title: string): Promise<void> {
  const clean = title.trim();
  if (!clean) return;
  await call(`/${encodeURIComponent(id)}/rename`, {
    method: "POST",
    body: JSON.stringify({ title: clean }),
  });
}

export async function archiveSession(id: string): Promise<void> {
  await call(`/${encodeURIComponent(id)}/archive`, { method: "POST" });
}

export async function clearAllSessions(): Promise<void> {
  await call("", { method: "DELETE" });
  try {
    localStorage.removeItem("ide-ai-recent");
  } catch {
    // storage unavailable
  }
}

export function newSessionId() {
  return `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`;
}

export function whenText(at: number) {
  const mins = Math.floor((Date.now() - at) / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}
