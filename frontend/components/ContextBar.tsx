"use client";

import { IconBranch } from "@/components/Icons";
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
  const dirty = (git?.files.length ?? 0) > 0;

  async function commit() {
    if (!msg.trim()) return;
    await onCommit(msg);
    setMsg("");
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
      <span className="spacer" />
      {dirty && (
        <>
          <input
            className="ctx-msg"
            value={msg}
            onChange={(e) => setMsg(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
            }}
            placeholder={`Commit ${git?.files.length} file${git?.files.length === 1 ? "" : "s"}`}
          />
          <button className="btn btn-sm" disabled={!msg.trim()} onClick={commit}>
            Commit
          </button>
        </>
      )}
    </div>
  );
}
