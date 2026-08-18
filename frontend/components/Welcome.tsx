"use client";

import { useEffect, useState } from "react";

const RECENT_KEY = "ide-ai-recent";

export type Recent = { name: string; path: string };

export function rememberRecent(path: string) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  const prev: Recent[] = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
  const next = [{ name, path }, ...prev.filter((p) => p.path !== path)].slice(0, 12);
  localStorage.setItem(RECENT_KEY, JSON.stringify(next));
}

export function Welcome({
  onPick,
  onOpenRecent,
}: {
  onPick: () => void;
  onOpenRecent: (path: string) => void;
}) {
  const [recent, setRecent] = useState<Recent[]>([]);

  useEffect(() => {
    try {
      setRecent(JSON.parse(localStorage.getItem(RECENT_KEY) || "[]"));
    } catch {
      setRecent([]);
    }
  }, []);

  return (
    <div className="welcome">
      <div className="welcome-inner">
        <h1 className="wordmark">IDE-AI</h1>
        <div className="tiles">
          <button className="tile" onClick={onPick}>
            <IconFolder />
            Open project
          </button>
          <button className="tile" onClick={onPick}>
            <IconClone />
            Clone repo
          </button>
          <button className="tile" disabled>
            <IconSsh />
            Connect via SSH
          </button>
        </div>
        <div className="recent-head">Recent projects</div>
        <div className="recent-list">
          {recent.length === 0 && <div className="recent-empty">No recent projects</div>}
          {recent.map((r) => (
            <button key={r.path} className="recent-row" onClick={() => onOpenRecent(r.path)}>
              <span>{r.name}</span>
              <span className="recent-path">{r.path}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

function IconFolder() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden>
      <path d="M3 8.5A2 2 0 0 1 5 6.5h4l2 2h8a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}

function IconClone() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden>
      <path d="M12 4v10" />
      <path d="M8 10l4 4 4-4" />
      <path d="M5 18h14" />
    </svg>
  );
}

function IconSsh() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden>
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <path d="M12 8v5l3 2" />
    </svg>
  );
}
