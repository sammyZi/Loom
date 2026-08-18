"use client";

import { api, type AgentEvent } from "@/lib/api";
import { useState } from "react";

export function AgentPanel({
  log,
  busy,
}: {
  log: { kind: string; text: string }[];
  busy: boolean;
}) {
  const [prompt, setPrompt] = useState("");
  const [err, setErr] = useState("");

  async function run() {
    setErr("");
    try {
      await api.runAgent(prompt);
      setPrompt("");
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="agent">
      <div className="head">Agent</div>
      <div className="log">
        {log.map((l, i) => (
          <div key={i} className={l.kind}>
            {l.text}
          </div>
        ))}
      </div>
      <div className="composer">
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Describe a task"
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) run();
          }}
        />
        {busy ? (
          <button onClick={() => api.cancelAgent()}>Cancel</button>
        ) : (
          <button onClick={run}>Run</button>
        )}
      </div>
      {err && <div className="err" style={{ padding: "0 8px 8px" }}>{err}</div>}
    </div>
  );
}

export function formatEvent(ev: AgentEvent): { kind: string; text: string } | null {
  switch (ev.type) {
    case "token":
      return { kind: "token", text: ev.text };
    case "tool_call":
      return { kind: "tool", text: `\n[${ev.name}] ${JSON.stringify(ev.input)}\n` };
    case "tool_result":
      return { kind: "tool", text: ev.output.slice(0, 4000) };
    case "status":
      return { kind: "ok", text: `\n— ${ev.message} —\n` };
    case "done":
      return { kind: "ok", text: `\n${ev.summary}\n` };
    case "error":
      return { kind: "err", text: `\nerror: ${ev.message}\n` };
    case "diff":
      return { kind: "tool", text: `\n[diff ${ev.path}]\n` };
    default:
      return null;
  }
}
