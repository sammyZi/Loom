"use client";

import { api, type AgentEvent } from "@/lib/api";
import { useState } from "react";

export const MODELS = [
  { id: "deepseek-v4-flash", label: "Flash" },
  { id: "deepseek-v4-pro", label: "Pro" },
] as const;

export function AgentPanel({
  log,
  busy,
  model,
  onModel,
}: {
  log: { kind: string; text: string }[];
  busy: boolean;
  model: string;
  onModel: (id: string) => void;
}) {
  const [prompt, setPrompt] = useState("");
  const [err, setErr] = useState("");

  async function run() {
    setErr("");
    try {
      await api.runAgent(prompt, model);
      setPrompt("");
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="agent">
      <div className="head">
        <span>Agent</span>
        <div className="seg">
          {MODELS.map((m) => (
            <button
              key={m.id}
              className={model === m.id ? "on" : ""}
              onClick={() => onModel(m.id)}
              disabled={busy}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>
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
          placeholder="Describe a change. Ctrl+Enter to run."
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) run();
          }}
        />
        <div className="composer-row">
          <span className="hint">{model === "deepseek-v4-flash" ? "V4 Flash · faster" : "V4 Pro · stronger"}</span>
          {busy ? (
            <button className="btn btn-danger" onClick={() => api.cancelAgent()}>
              Stop
            </button>
          ) : (
            <button className="btn btn-primary" onClick={run}>
              Run
            </button>
          )}
        </div>
      </div>
      {err && (
        <div className="err" style={{ margin: "0 10px 10px" }}>
          {err}
        </div>
      )}
    </div>
  );
}

export function formatEvent(ev: AgentEvent): { kind: string; text: string } | null {
  switch (ev.type) {
    case "token":
      return { kind: "token", text: ev.text };
    case "think":
      return { kind: "think", text: ev.text };
    case "tool_call":
      return { kind: "tool", text: `${ev.name}  ${JSON.stringify(ev.input)}` };
    case "tool_result":
      return { kind: "tool", text: ev.output.slice(0, 4000) };
    case "status":
      return { kind: "phase", text: ev.message };
    case "done":
      return { kind: "ok", text: ev.summary };
    case "error":
      return { kind: "err", text: ev.message };
    case "diff":
      return { kind: "tool", text: `diff  ${ev.path}` };
    default:
      return null;
  }
}
