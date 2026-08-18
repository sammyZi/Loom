"use client";

import { IconFolder, IconMark, IconPanel, IconPlus, IconTrash } from "@/components/Icons";
import type { Recent } from "@/components/Welcome";
import { baseName } from "@/lib/log";
import { whenText, type Session } from "@/lib/store";

export function Sidebar({
  open,
  folder,
  recent,
  sessions,
  activeId,
  title,
  busy,
  onToggle,
  onNewTask,
  onPick,
  onOpenRecent,
  onLoadSession,
  onDeleteSession,
}: {
  open: boolean;
  folder: string;
  recent: Recent[];
  sessions: Session[];
  activeId: string | null;
  title: string;
  busy: boolean;
  onToggle: () => void;
  onNewTask: () => void;
  onPick: () => void;
  onOpenRecent: (path: string) => void;
  onLoadSession: (s: Session) => void;
  onDeleteSession: (id: string) => void;
}) {
  const others = recent.filter((r) => r.path !== folder);
  const past = sessions.filter((s) => s.id !== activeId);

  return (
    <aside className={`sidebar ${open ? "" : "off"}`}>
      <div className="side-top">
        <IconMark />
        <span className="side-name">IDE-AI</span>
        <span className="spacer" />
        <button className="icon-btn" title="Hide sidebar" onClick={onToggle}>
          <IconPanel />
        </button>
      </div>

      <div className="side-nav">
        <button onClick={onNewTask}>
          <IconPlus />
          New task
        </button>
        <button onClick={onPick}>
          <IconFolder />
          Open folder
        </button>
      </div>

      <div className="side-scroll">
        <div className="side-label">{baseName(folder)}</div>
        <div className="sess on">
          <span className={`dot ${busy ? "run" : ""}`} />
          <span className="sess-title">{title}</span>
        </div>

        {past.length > 0 && <div className="side-label">History</div>}
        {past.map((s) => (
          <div className="sess" key={s.id}>
            <span className="dot off" />
            <button className="sess-open" onClick={() => onLoadSession(s)} title={s.title}>
              <span className="sess-title">{s.title}</span>
              <span className="sess-when">{whenText(s.at)}</span>
            </button>
            <button
              className="icon-btn sess-del"
              title="Delete session"
              onClick={() => onDeleteSession(s.id)}
            >
              <IconTrash />
            </button>
          </div>
        ))}

        {others.length > 0 && <div className="side-label">Projects</div>}
        {others.map((r) => (
          <button className="sess" key={r.path} title={r.path} onClick={() => onOpenRecent(r.path)}>
            <span className="dot off" />
            <span className="sess-title">{r.name}</span>
          </button>
        ))}
      </div>

      <div className="side-foot" title={folder}>
        <IconFolder />
        <span className="folder-chip">{folder}</span>
      </div>
    </aside>
  );
}
