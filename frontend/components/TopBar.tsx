"use client";

import {
  IconDiff,
  IconKebab,
  IconFolder,
  IconGlobe,
  IconMark,
  IconPanel,
  IconTerminal,
} from "@/components/Icons";

export type Panel = "none" | "terminal" | "diff" | "browser";

/**
 * Window-wide title bar, Claude Code's layout: app controls over the sidebar
 * on the left, the session title and its folder chip, panel toggles on the
 * right. It spans the sidebar too, so the title lines up with the content.
 */
export function TopBar({
  title,
  project,
  sideOpen,
  sideW,
  panel,
  live = true,
  onShowSide,
  onPanel,
  onOpenFolder,
  onSettings,
}: {
  title: string;
  project: string;
  sideOpen: boolean;
  /** Sidebar width, so the title starts where the content does. */
  sideW: number;
  panel: Panel;
  /** Backend socket connectivity. Only surfaced when it is broken. */
  live?: boolean;
  onShowSide: () => void;
  onPanel: (p: Panel) => void;
  onOpenFolder: () => void;
  onSettings: () => void;
}) {
  const toggle = (p: Panel) => onPanel(panel === p ? "none" : p);
  return (
    <div className="titlebar">
      <div
        className={`titlebar-left ${sideOpen ? "with-side" : ""}`}
        style={{ width: sideOpen ? sideW : undefined }}
      >
        <span className="titlebar-mark" aria-hidden>
          <IconMark />
        </span>
        <button
          className={`icon-btn ${sideOpen ? "on" : ""}`}
          title={sideOpen ? "Hide sidebar" : "Show sidebar"}
          onClick={onShowSide}
        >
          <IconPanel />
        </button>
      </div>
      <div className="titlebar-main">
        <span className="topbar-title">{title}</span>
        {project && <span className="chip">{project}</span>}
        {/* Connected is the normal state and needs no badge; only the broken
            one is worth a word, so the bar stays quiet until something is. */}
        {!live && (
          <span className="chip chip-dead" title="Reconnecting to the backend…">
            reconnecting
          </span>
        )}
        <span className="spacer" />
        <button
          className={`icon-btn ${panel === "terminal" ? "on" : ""}`}
          title={panel === "terminal" ? "Hide terminal" : "Show terminal"}
          onClick={() => toggle("terminal")}
        >
          <IconTerminal />
        </button>
        <button
          className={`icon-btn ${panel === "diff" ? "on" : ""}`}
          title={panel === "diff" ? "Hide changes" : "Show changes"}
          onClick={() => toggle("diff")}
        >
          <IconDiff />
        </button>
        <button
          className={`icon-btn ${panel === "browser" ? "on" : ""}`}
          title={panel === "browser" ? "Hide browser" : "Show browser"}
          onClick={() => toggle("browser")}
        >
          <IconGlobe />
        </button>
        <button className="icon-btn" title="Open folder" onClick={onOpenFolder}>
          <IconFolder />
        </button>
        <button className="icon-btn" title="Settings" onClick={onSettings}>
          <IconKebab />
        </button>
      </div>
    </div>
  );
}
