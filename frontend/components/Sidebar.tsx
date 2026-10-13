"use client";

import {
  IconArchive,
  IconBook,
  IconChevron,
  IconClose,
  IconFolder,
  IconGear,
  IconPlus,
  IconDots,
  IconSearch,
  IconTrash,
  IconUndo,
} from "@/components/Icons";
import { api, type SessionLite, type SkillInfo } from "@/lib/api";
import type { Recent } from "@/components/Welcome";
import { baseName } from "@/lib/log";
import { whenText, type Session } from "@/lib/store";
import { useEffect, useState } from "react";

export type Group = { path: string; name: string; sessions: Session[] };

export function Sidebar({
  open,
  folder,
  recent,
  groups,
  activeId,
  title,
  busy,
  archiveOpen = false,
  archived = [],
  onToggleArchiveView,
  onUnarchive,
  onLoadArchivedSession,
  onToggle,
  onNewTask,
  onPick,
  onOpenRecent,
  onLoadSession,
  onDeleteSession,
  onRenameSession,
  onArchiveSession,
  onClearAll,
  onOpenSettings,
}: {
  open: boolean;
  folder: string;
  recent: Recent[];
  groups: Group[];
  activeId: string | null;
  title: string;
  busy: boolean;
  /** The collapsible archive drawer under the project list. */
  archiveOpen?: boolean;
  archived?: SessionLite[];
  onToggleArchiveView?: () => void;
  onUnarchive?: (id: string) => void;
  onLoadArchivedSession?: (s: SessionLite) => void;
  onToggle: () => void;
  onNewTask: () => void;
  onPick: () => void;
  /** `sessionId` resumes that transcript after the folder opens. */
  onOpenRecent: (path: string, sessionId?: string) => void;
  onLoadSession: (s: Session) => void;
  onDeleteSession: (id: string) => void;
  onRenameSession: (id: string, title: string) => void;
  onArchiveSession: (id: string) => void;
  onClearAll: () => void;
  /** Providers & API keys; lives beside Clear all rather than in the top bar. */
  onOpenSettings: () => void;
}) {
  const [shut, setShut] = useState<Record<string, boolean>>({});
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [skillsOpen, setSkillsOpen] = useState(false);
  const [skills, setSkills] = useState<SkillInfo[]>([]);

  // Re-read when the folder changes: project skills live inside the workspace.
  useEffect(() => {
    api
      .skills()
      .then((r) => setSkills(r.skills))
      .catch(() => setSkills([]));
  }, [folder]);
  const [query, setQuery] = useState("");

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

  /** Case-insensitive filter over the session title and its transcript text. */
  function matches(texts: string[]) {
    const q = query.trim().toLowerCase();
    if (!q) return true;
    return texts.some((t) => t.toLowerCase().includes(q));
  }
  const sessionText = (log: unknown[] | undefined) =>
    (Array.isArray(log) ? log : [])
      .map((l) => {
        const o = l as { text?: unknown };
        return typeof o?.text === "string" ? o.text : "";
      })
      .join("\n");

  const visibleGroups = groups
    .map((g) => ({
      ...g,
      sessions: g.sessions.filter((s) =>
        matches([s.title, s.folder, sessionText(s.log as unknown[])]),
      ),
    }))
    .filter((g) => g.path === folder || g.sessions.length > 0);
  const qActive = query.trim().length > 0;
  const visibleArchived = archived.filter((s) =>
    matches([s.title, s.folder, sessionText(s.log)]),
  );

  return (
    <aside className={`sidebar ${open ? "" : "off"}`}>
      {menuFor && <div className="menu-backdrop" onClick={() => setMenuFor(null)} />}

      <div className="side-search">
        <IconSearch />
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search"
          spellCheck={false}
          aria-label="Search chats"
        />
        {qActive && (
          <button className="icon-btn side-search-x" title="Clear search" onClick={() => setQuery("")}>
            <IconClose />
          </button>
        )}
      </div>

      <div className="side-nav">
        <button onClick={onNewTask} title="New task">
          <IconPlus />
          New
        </button>
        <button onClick={onPick}>
          <IconFolder />
          Open folder
        </button>
      </div>

      {/* What the agent can load for a task. Read-only: skills are files on
          disk, so this shows what is installed and where. */}
      <div className="proj proj-archived">
        <div className="proj-head">
          <button
            className="proj-name arch-toggle"
            onClick={() => setSkillsOpen((v) => !v)}
            aria-expanded={skillsOpen}
            title="Skills the agent can load"
          >
            <IconChevron className={`proj-chev ${skillsOpen ? "" : "down"}`} />
            <IconBook className="arch-ico" />
            Skills
            {skills.length > 0 && <span className="arch-count">{skills.length}</span>}
          </button>
        </div>
        {skillsOpen && (
          <div className="proj-body arch-body">
            {skills.length === 0 && (
              <div className="sess-empty">
                None installed. Drop a folder with a SKILL.md into
                <code> .opencode/skills/</code>.
              </div>
            )}
            {skills.map((s) => (
              <div className="skill-row" key={s.name} title={s.path}>
                <span className="skill-name">{s.name}</span>
                <span className="skill-desc">{s.description}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Above the search box: archived chats are a fixed drawer, not another
          project in the scrolling list below. */}
      <div className="proj proj-archived">
        <div className="proj-head">
          <button
            className="proj-name arch-toggle"
            onClick={() => onToggleArchiveView?.()}
            aria-expanded={archiveOpen}
            title="Show archived chats"
          >
            <IconChevron className={`proj-chev ${archiveOpen ? "" : "down"}`} />
            <IconArchive className="arch-ico" />
            Archived
            {archived.length > 0 && <span className="arch-count">{archived.length}</span>}
          </button>
        </div>
        {archiveOpen && (
          <div className="proj-body arch-body">
            {visibleArchived.length === 0 && <div className="sess-empty">No archived chats.</div>}
            {visibleArchived.map((s) => (
              <div className="sess" key={s.id}>
                <span className="dot off" />
                <button
                  className="sess-open"
                  title={`${s.title}\n${whenText(s.at)}\n${s.folder}`}
                  onClick={() => onLoadArchivedSession?.(s)}
                >
                  <span className="sess-title">{s.title}</span>
                </button>
                <div className="sess-menu-wrap">
                  <button
                    className="icon-btn sess-dots"
                    title="Archived chat actions"
                    onClick={() => setMenuFor(menuFor === `a-${s.id}` ? null : `a-${s.id}`)}
                  >
                    <IconDots />
                  </button>
                  {menuFor === `a-${s.id}` && (
                    <div className="menu">
                      <button
                        onClick={() => {
                          setMenuFor(null);
                          onUnarchive?.(s.id);
                        }}
                      >
                        <IconUndo className="menu-ico" />
                        Unarchive
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
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="side-scroll">
        {visibleGroups.map((g) => {
          const current = normEq(g.path, folder);
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
                            title={`${s.title}\n${whenText(s.at)}${
                              current ? "" : "\nOpens this project and resumes the session"
                            }`}
                            onClick={() =>
                              current ? onLoadSession(s) : onOpenRecent(g.path, s.id)
                            }
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

                  {/* Only while searching: with no query an empty project just
                      has no chats yet, and "No chats match." read as an error. */}
                  {g.sessions.length === 0 && current && qActive && (
                    <div className="sess-empty">No chats match.</div>
                  )}
                </div>
              )}
            </div>
          );
        })}

        {qActive && !visibleGroups.some((g) => g.sessions.length > 0) && (
          <div className="no-hits">Nothing found for “{query.trim()}”.</div>
        )}

      </div>

      <div className="side-foot">
        <div className="side-foot-row">
          <div className="foot-who-wrap">
            <button
              className="foot-who"
              onClick={() => setMenuFor(menuFor === "foot" ? null : "foot")}
              aria-expanded={menuFor === "foot"}
              title="Loom menu"
            >
              <span className="foot-avatar" aria-hidden>
                L
              </span>
              <span className="foot-name">Loom</span>
              <span className="foot-plan">· Local</span>
              <IconChevron className={`foot-chev ${menuFor === "foot" ? "up" : ""}`} />
            </button>
            {menuFor === "foot" && (
              <div className="menu menu-up">
                <button
                  onClick={() => {
                    setMenuFor(null);
                    onOpenSettings();
                  }}
                >
                  <IconGear className="menu-ico" />
                  Providers &amp; API keys
                </button>
                <button
                  className="menu-danger"
                  onClick={() => {
                    setMenuFor(null);
                    // A dialog, not a click-again toggle: a timed arm could be
                    // pressed all day a moment too late and never fire. It
                    // also says what is about to be destroyed.
                    const total =
                      groups.reduce((n, g) => n + g.sessions.length, 0) + archived.length;
                    if (total === 0) return;
                    const ok = window.confirm(
                      `Delete all ${total} stored session${total === 1 ? "" : "s"}, ` +
                        `across every project?\n\nThis cannot be undone. Your API keys ` +
                        `and settings are not affected.`,
                    );
                    if (ok) onClearAll();
                  }}
                >
                  <IconTrash className="menu-ico" />
                  Clear all sessions
                </button>
              </div>
            )}
          </div>
          <button
            className="foot-gear"
            onClick={onOpenSettings}
            title="Providers & API keys"
            aria-label="Providers and API keys"
          >
            <IconGear />
          </button>
        </div>
      </div>
    </aside>
  );
}

function normEq(a: string, b: string) {
  return a.toLowerCase().replace(/[\\/]+/g, "/") === b.toLowerCase().replace(/[\\/]+/g, "/");
}

/**
 * Windows paths are case-insensitive and mix separators, so grouping compares
 * a normalized key: without this the same project could appear as two groups
 * ("D:\X" vs "d:/x") and split its sessions across both.
 */
export function normPath(p: string) {
  return p.toLowerCase().replace(/[\\/]+/g, "/");
}

/** Current project first, then every other project we know about. */
export function buildGroups(folder: string, recent: Recent[], all: Session[]): Group[] {
  const paths = [folder, ...recent.map((r) => r.path), ...all.map((s) => s.folder)];
  const seen = new Set<string>();
  const out: Group[] = [];
  for (const path of paths) {
    if (!path) continue;
    const key = normPath(path);
    if (seen.has(key)) continue;
    seen.add(key);
    // already ordered by lib/store; keep that order so rows never move
    const sessions = all.filter((s) => normPath(s.folder) === key);
    // Only the open folder gets a group unconditionally. Other projects appear
    // once they actually have history, so clearing sessions empties the list.
    if (path !== folder && sessions.length === 0) continue;
    out.push({ path, name: baseName(path), sessions });
  }
  return out;
}
