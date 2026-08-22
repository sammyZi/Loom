"use client";

import { IconBranch, IconCheck } from "@/components/Icons";
import { useState } from "react";

export type Git = { branch: string; files: { path: string; status: string }[] };

export function ContextBar({
  project,
  git,
  stat,
  onCommit,
}: {
  project: string;
  git: Git | null;
  stat: { add: number; del: number };
  onCommit: (message: string) => Promise<void>;
}) {
  const [msg, setMsg] = useState("");
  const [phase, setPhase] = useState<"idle" | "busy" | "done">("idle");
  const files = git?.files.length ?? 0;
  const dirty = files > 0;
  const label = `${files} file${files === 1 ? "" : "s"}`;

  async function commit() {
    const message = msg.trim();
    if (!message || phase === "busy") return;
    setPhase("busy");
    try {
      await onCommit(message);
      setMsg("");
      setPhase("done");
      setTimeout(() => setPhase("idle"), 1600);
    } catch {
      // The page reports the error; keep the message so it can be retried.
      setPhase("idle");
    }
  }

  return (
    <div className="ctx">
      <span className="ctx-repo">{project}</span>
      <span className="ctx-branch">
        <IconBranch />
        {git?.branch || "no repo"}
      </span>
      {(stat.add > 0 || stat.del > 0) && (
        <span className="ctx-stat">
          <b className="stat-add">+{stat.add}</b>
          <b className="stat-del">-{stat.del}</b>
        </span>
      )}
      {/* No spacer: it and .ctx-commit both grew, so they split the free space
          and squeezed the message field down to a couple of characters. The
          commit block alone takes the slack now, via margin-left:auto. */}
      {phase === "done" && (
        <span className="ctx-done">
          <IconCheck />
          Committed
        </span>
      )}
      {dirty && phase !== "done" && (
        <div className="ctx-commit">
          <span className="ctx-count" title={git?.files.map((f) => f.path).join("\n")}>
            {label}
          </span>
          <input
            className="ctx-msg"
            value={msg}
            disabled={phase === "busy"}
            onChange={(e) => setMsg(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setMsg("");
            }}
            placeholder="Commit message"
            aria-label={`Commit message for ${label}`}
          />
          <button
            className="btn btn-sm btn-primary"
            disabled={!msg.trim() || phase === "busy"}
            onClick={commit}
          >
            {phase === "busy" ? "Committing…" : "Commit"}
          </button>
        </div>
      )}
    </div>
  );
}
