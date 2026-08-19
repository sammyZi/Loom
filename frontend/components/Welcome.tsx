"use client";

import { IconBack, IconClone, IconFolder, IconMark, IconSsh } from "@/components/Icons";
import { ShaderBackground } from "@/components/ui/manu";
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
  onCancel,
}: {
  onPick: () => void;
  onOpenRecent: (path: string) => void;
  /** Shown only when a workspace is already open, so the screen is escapable. */
  onCancel?: () => void;
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
      <ShaderBackground className="welcome-shader" />
      <div className="welcome-scrim" />
      {onCancel && (
        <button className="welcome-back" onClick={onCancel}>
          <IconBack />
          Back to workspace
        </button>
      )}
      <div className="welcome-inner">
        <div className="welcome-head">
          <IconMark />
          <h1 className="wordmark">Loom</h1>
          <p className="welcome-tag">A coding agent that works directly in your local repos</p>
        </div>
        <div className="tiles">
          <button className="tile tile-primary" onClick={onPick}>
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
            <span className="tile-soon">Soon</span>
          </button>
        </div>
        {recent.length > 0 && (
          <div className="recent-list">
            <div className="recent-head">Recent projects</div>
            {recent.map((r) => (
              <button key={r.path} className="recent-row" onClick={() => onOpenRecent(r.path)}>
                <span>{r.name}</span>
                <span className="recent-path">{r.path}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
