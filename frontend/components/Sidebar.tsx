"use client";

import {
  IconChevron,
  IconFolder,
  IconMark,
  IconPanel,
  IconPlus,
  IconDots,
  IconTrash,
} from "@/components/Icons";
import type { Recent } from "@/components/Welcome";
import { baseName } from "@/lib/log";
import { whenText, type Session } from "@/lib/store";
import { useState } from "react";

export type Group = { path: string; name: string; sessions: Session[] };

export function Sidebar({
  open,
  folder,
  recent,
  groups,
  activeId,
  title,
  busy,
  onToggle,
  onNewTask,
  onPick,
  onOpenRecent,
  onLoadSession,
  onDeleteSession,
  onRenameSession,
  onArchiveSession,
  onClearAll,
}: {
  open: boolean;
  folder: string;
  recent: Recent[];
  groups: Group[];
  activeId: string | null;
  title: string;
  busy: boolean;
  onToggle: () => void;
  onNewTask: () => void;
  onPick: () => void;
  onOpenRecent: (path: string) => void;
  onLoadSession: (s: Session) => void;
  onDeleteSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onArchiveSession: (id: string) => void;
  onClearAll: () => void;
}) {
  const [shut, setShut] = useState<Record<string, boolean>>({});
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [confirmClear, setConfirmClear] = useState(false);

  const flip = (path: string) => setShut((p) => ({ ...p, [path]: !p[path] }));

  function startRename(s: Session) {
    setMenuFor(null);
    setRenaming(s.id);
    setDraft(s.title);
  }

  function commitRename(id: string) {
    onRenameSession(id, draft);
    setRenaming(null);
  }

  return (
    <aside className={`sidebar ${open ? "" : "off"}`}>
      {menuFor && <div className="menu-backdrop" onClick={() => setMenuFor(null)} />}

      <div className="side-top">
        <IconMark />
        <span className="side-name">Loom</span>
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
        {groups.map((g) => {
          const current = g.path === folder;
          const shutNow = shut[g.path] === true;
          return (
            <div className="proj" key={g.path}>
              <div className="proj-head">
                <button
                  className="proj-name"
                  onClick={() => flip(g.path)}
                  aria-expanded={!shutNow}
                  title={g.path}
                >
                  <IconChevron className={`proj-chev ${shutNow ? "" : "down"}`} />
                  {g.name}
                </button>
                <button
                  className="icon-btn proj-add"
                  title={current ? "New task" : `Open ${g.name} and start a task`}
                  onClick={() => (current ? onNewTask() : onOpenRecent(g.path))}
                >
                  <IconPlus />
                </button>
              </div>

              {!shutNow && (
                <div className="proj-body">
                  {/* Unsaved draft only; saved sessions keep their place in the list below. */}
                  {current && !activeId && (
                    <div className="sess on">
                      <span className={`dot ${busy ? "run" : ""}`} />
                      <span className="sess-title">{title}</span>
                    </div>
                  )}

                  {g.sessions.map((s) => {
                    const isActive = s.id === activeId;
                    return (
                      <div className={`sess ${isActive ? "on" : ""}`} key={s.id}>
                        <span className={`dot ${isActive && busy ? "run" : isActive ? "" : "off"}`} />

                        {renaming === s.id ? (
                          <input
                            className="sess-rename"
                            autoFocus
                            value={draft}
                            onChange={(e) => setDraft(e.target.value)}
                            onBlur={() => commitRename(s.id)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") commitRename(s.id);
                              if (e.key === "Escape") setRenaming(null);
                            }}
                          />
                        ) : (
                          <button
                            className="sess-open"
                            title={`${s.title}
${whenText(s.at)}`}
                            onClick={() => (current ? onLoadSession(s) : onOpenRecent(g.path))}
                          >
                            <span className="sess-title">{isActive ? title : s.title}</span>
                          </button>
                        )}

                        {current && renaming !== s.id && (
                          <div className="sess-menu-wrap">
                            <button
                              className="icon-btn sess-dots"
                              title="Session actions"
                              onClick={() => setMenuFor(menuFor === s.id ? null : s.id)}
                            >
                              <IconDots />
                            </button>
                            {menuFor === s.id && (
                              <div className="menu">
                                <button onClick={() => startRename(s)}>Rename</button>
                                <button
                                  onClick={() => {
                                    setMenuFor(null);
                                    onArchiveSession(s.id);
                                  }}
                                >
                                  Archive
                                </button>
                                <button
                                  className="menu-danger"
                                  onClick={() => {
                                    setMenuFor(null);
                                    onDeleteSession(s.id);
                                  }}
                                >
                                  Delete
                                </button>
                              </div>
                            )}
                          </div>
                        )}
                      </div>
                    );
                  })}

                </div>
              )}
            </div>
          );
        })}
      </div>

      <div className="side-foot">
        <button
          className={`clear-all ${confirmClear ? "armed" : ""}`}
          onClick={() => {
            if (!confirmClear) {
              setConfirmClear(true);
              setTimeout(() => setConfirmClear(false), 4000);
              return;
            }
            setConfirmClear(false);
            onClearAll();
          }}
          title="Delete every stored session, for all projects"
        >
          <IconTrash />
          {confirmClear ? "Click again to delete all" : "Clear all sessions"}
        </button>
      </div>
    </aside>
  );
}

/** Current project first, then every other project we know about. */
export function buildGroups(folder: string, recent: Recent[], all: Session[]): Group[] {
  const paths = [folder, ...recent.map((r) => r.path), ...all.map((s) => s.folder)];
  const seen = new Set<string>();
  const out: Group[] = [];
  for (const path of paths) {
    if (!path || seen.has(path)) continue;
    seen.add(path);
    // already ordered by lib/store; keep that order so rows never move
    const sessions = all.filter((s) => s.folder === path);
    // Only the open folder gets a group unconditionally. Other projects appear
    // once they actually have history, so clearing sessions empties the list.
    if (path !== folder && sessions.length === 0) continue;
    out.push({ path, name: baseName(path), sessions });
  }
  return out;
}
