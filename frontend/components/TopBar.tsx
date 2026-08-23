"use client";

import {
  IconCheck,
  IconDiff,
  IconGlobe,
  IconLink,
  IconPanel,
  IconTerminal,
} from "@/components/Icons";

export type Panel = "none" | "terminal" | "diff" | "browser";

export function TopBar({
  title,
  project,
  sideOpen,
  panel,
  copied,
  live = true,
  onShowSide,
  onPanel,
  onCopyLink,
}: {
  title: string;
  project: string;
  sideOpen: boolean;
  panel: Panel;
  copied: boolean;
  /** Backend socket connectivity; a red dot warns that the feed is stale. */
  live?: boolean;
  onShowSide: () => void;
  onPanel: (p: Panel) => void;
  onCopyLink: () => void;
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
      <span
        className={`chip ${live ? "chip-live" : "chip-dead"}`}
        title={live ? "Connected to the Loom backend" : "Reconnecting to the backend…"}
      >
        {live ? "connected" : "reconnecting"}
      </span>
      <span className="spacer" />
      <button className="icon-btn" title="Copy link" onClick={onCopyLink}>
        {copied ? <IconCheck /> : <IconLink />}
      </button>
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
