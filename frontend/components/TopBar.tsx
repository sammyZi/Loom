"use client";

import {
  IconDiff,
  IconGlobe,
  IconPanel,
  IconTerminal,
} from "@/components/Icons";

export type Panel = "none" | "terminal" | "diff" | "browser";

export function TopBar({
  title,
  project,
  sideOpen,
  panel,
  live = true,
  onShowSide,
  onPanel,
}: {
  title: string;
  project: string;
  sideOpen: boolean;
  panel: Panel;
  /** Backend socket connectivity. Only surfaced when it is broken. */
  live?: boolean;
  onShowSide: () => void;
  onPanel: (p: Panel) => void;
}) {
  return (
    <div className="topbar">
      {!sideOpen && (
        <button className="icon-btn" title="Show sidebar" onClick={onShowSide}>
          <IconPanel />
        </button>
      )}
      <span className="topbar-title">{title}</span>
      <span className="chip">{project}</span>
      {/* Connected is the normal state and needs no badge; only the broken
          one is worth a word, so the bar stays quiet until something is. */}
      {!live && (
        <span className="chip chip-dead" title="Reconnecting to the backend…">
          reconnecting
        </span>
      )}
      <span className="spacer" />
      <button
        className={`icon-btn ${panel === "diff" ? "on" : ""}`}
        title={panel === "diff" ? "Hide changes" : "Show changes"}
        onClick={() => onPanel(panel === "diff" ? "none" : "diff")}
      >
        <IconDiff />
      </button>
      <button
        className={`icon-btn ${panel === "browser" ? "on" : ""}`}
        title={panel === "browser" ? "Hide browser" : "Show browser"}
        onClick={() => onPanel(panel === "browser" ? "none" : "browser")}
      >
        <IconGlobe />
      </button>
      <button
        className={`icon-btn ${panel === "terminal" ? "on" : ""}`}
        title={panel === "terminal" ? "Hide terminal" : "Show terminal"}
        onClick={() => onPanel(panel === "terminal" ? "none" : "terminal")}
      >
        <IconTerminal />
      </button>
    </div>
  );
}
