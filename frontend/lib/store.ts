import type { LogItem } from "./log";

const KEY = "ide-ai-sessions";
const MAX = 40;

export type Session = {
  id: string;
  folder: string;
  title: string;
  log: LogItem[];
  at: number;
};

// ponytail: localStorage caps around 5MB; move to IndexedDB if long transcripts start getting dropped
function read(): Session[] {
  try {
    const all = JSON.parse(localStorage.getItem(KEY) || "[]") as Session[];
    return Array.isArray(all) ? all : [];
  } catch {
    return [];
  }
}

function write(all: Session[]): Session[] {
  let next = all.sort((a, b) => b.at - a.at).slice(0, MAX);
  // On quota errors keep dropping the oldest half until it fits; never throw at the caller,
  // losing old history beats losing the run that just finished.
  while (next.length) {
    try {
      localStorage.setItem(KEY, JSON.stringify(next));
      return next;
    } catch {
      next = next.slice(0, Math.floor(next.length / 2));
    }
  }
  try {
    localStorage.setItem(KEY, "[]");
  } catch {
    // storage unavailable entirely
  }
  return [];
}

export function loadSessions(folder: string): Session[] {
  return read().filter((s) => s.folder === folder);
}

export function saveSession(s: Session): Session[] {
  const all = read().filter((x) => x.id !== s.id);
  write([...all, s]);
  return loadSessions(s.folder);
}

export function deleteSession(id: string, folder: string): Session[] {
  write(read().filter((s) => s.id !== id));
  return loadSessions(folder);
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
