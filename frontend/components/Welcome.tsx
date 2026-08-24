"use client";

import { IconBack, IconClone, IconFolder, IconMark, IconSsh } from "@/components/Icons";
import { ShaderBackground } from "@/components/ui/manu";
import { api } from "@/lib/api";
import { baseName } from "@/lib/log";
import { useEffect, useState } from "react";

const RECENT_KEY = "ide-ai-recent";

export type Recent = { name: string; path: string };

export function rememberRecent(path: string) {
  const prev: Recent[] = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
  const next = [{ name: baseName(path), path }, ...prev.filter((p) => p.path !== path)].slice(0, 12);
  localStorage.setItem(RECENT_KEY, JSON.stringify(next));
}

export function readRecent(): Recent[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
  } catch {
    return [];
  }
}

/**
 * Drops recents whose folder is gone. A deleted or moved project used to sit
 * in the list forever with no way to clear it, and clicking it only produced
 * an error. Returns the surviving list, having written it back.
 */
export async function pruneRecent(): Promise<Recent[]> {
  const prev = readRecent();
  if (prev.length === 0) return prev;
  const alive = await Promise.all(prev.map((r) => api.dirExists(r.path)));
  const next = prev.filter((_, i) => alive[i]);
  if (next.length !== prev.length) localStorage.setItem(RECENT_KEY, JSON.stringify(next));
  return next;
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
    setRecent(readRecent());
    // Then drop any whose folder no longer exists, so a deleted project
    // stops being offered.
    pruneRecent().then(setRecent).catch(() => {});
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
