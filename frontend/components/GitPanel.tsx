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
    <div className="git-wrap">
      <div className="head">
        <span>Source control {status ? `· ${status.branch}` : ""}</span>
      </div>
      <div className="scroll">
        {status?.files.map((f) => (
          <div className="git-item" key={f.path} onClick={() => onSelect(f.path)}>
            <span>{f.path}</span>
            <span className={`st st-${f.status}`}>{f.status}</span>
          </div>
        ))}
        {status && status.files.length === 0 && <div className="git-empty">Working tree clean</div>}
      </div>
      <div className="commit">
        <input value={msg} onChange={(e) => setMsg(e.target.value)} placeholder="Commit message" />
        <button className="btn btn-primary" onClick={commit}>
          Commit
        </button>
      </div>
      {err && (
        <div className="err" style={{ margin: "0 10px 10px" }}>
          {err}
        </div>
      )}
    </div>
  );
}
