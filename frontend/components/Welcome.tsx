"use client";

import { IconClone, IconFolder, IconSsh } from "@/components/Icons";
import { baseName } from "@/lib/log";
import { useEffect, useState } from "react";

const RECENT_KEY = "ide-ai-recent";

export type Recent = { name: string; path: string };

export function rememberRecent(path: string) {
  const prev: Recent[] = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
  const next = [{ name: baseName(path), path }, ...prev.filter((p) => p.path !== path)].slice(0, 12);
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
