"use client";

import { api } from "@/lib/api";
import { useState } from "react";

export function GitPanel({
  status,
  onSelect,
  onRefresh,
}: {
  status: { branch: string; files: { path: string; status: string }[] } | null;
  onSelect: (path: string) => void;
  onRefresh: () => void;
}) {
  const [msg, setMsg] = useState("");
  const [err, setErr] = useState("");
  async function commit() {
    setErr("");
    try {
      await api.commit(msg);
      setMsg("");
      onRefresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }
  return (
    <div className="col" style={{ maxHeight: 220, borderTop: "1px solid var(--border)" }}>
      <div className="head">Git {status ? `· ${status.branch}` : ""}</div>
      <div className="scroll">
        {status?.files.map((f) => (
          <div className="git-item" key={f.path} onClick={() => onSelect(f.path)}>
            <span>{f.path}</span>
            <span className="status">{f.status}</span>
          </div>
        ))}
        {status && status.files.length === 0 && (
          <div style={{ padding: 10, color: "var(--muted)", fontSize: 12 }}>clean</div>
        )}
      </div>
      <div className="commit">
        <input value={msg} onChange={(e) => setMsg(e.target.value)} placeholder="commit message" />
        <button onClick={commit}>Commit</button>
      </div>
      {err && <div className="err" style={{ padding: "0 8px 8px" }}>{err}</div>}
    </div>
  );
}
