"use client";

import { IconClose, IconMaximize, IconPlus } from "@/components/Icons";
import { api, type AgentEvent } from "@/lib/api";
import { errText, toolLabel } from "@/lib/log";
import { useEffect, useRef, useState } from "react";

type Term = { id: number; name: string; log: string };

let nextId = 1;

/**
 * Lowest unused "Terminal N". Naming off the tab count reused numbers: with
 * tabs 1,2,3, closing 1 and 2 made the next tab collide with the surviving one.
 */
export function nextName(terms: { name: string }[]): string {
  const used = new Set(terms.map((t) => t.name));
  for (let i = 1; ; i++) {
    const name = `Terminal ${i}`;
    if (!used.has(name)) return name;
  }
}

/**
 * Split a log into runs, flagging the `cwd> cmd` lines run() echoes so they can
 * be coloured like the live prompt instead of inheriting the output colour.
 * Consecutive output lines stay in one run, so a long log renders as a handful
 * of nodes rather than one span per line.
 */
export function splitLog(log: string, cwd: string): { prompt: boolean; text: string }[] {
  const echo = `${cwd}> `;
  const lines = log.split("\n");
  const runs: { prompt: boolean; text: string }[] = [];
  let buf = "";
  for (let i = 0; i < lines.length; i++) {
    const nl = i < lines.length - 1 ? "\n" : "";
    if (lines[i].startsWith(echo)) {
      if (buf) {
        runs.push({ prompt: false, text: buf });
        buf = "";
      }
      runs.push({ prompt: true, text: lines[i] + nl });
    } else {
      buf += lines[i] + nl;
    }
  }
  if (buf) runs.push({ prompt: false, text: buf });
  return runs;
}

/**
 * Fold one agent event into the Agent tab's log, ignoring everything that is
 * not shell work. Kept beside splitLog because both depend on the `cwd> ` echo
 * format run() writes — if one changes the other must follow.
 */
export function appendAgentLog(prev: string, ev: AgentEvent, cwd: string): string {
  if (ev.type === "tool_call" && ev.name === "run_command") {
    return `${prev}${cwd}> ${toolLabel(ev.name, ev.input).detail}\n`;
  }
  if (ev.type === "tool_result" && ev.name === "run_command") {
    if (!ev.output) return prev;
    return prev + (ev.output.endsWith("\n") ? ev.output : `${ev.output}\n`);
  }
  return prev;
}

/** Log body shared by the agent section and the interactive terminals. */
function LogView({ log, cwd }: { log: string; cwd: string }) {
  return (
    <pre className="term-out">
      {splitLog(log, cwd).map((run, i) =>
        run.prompt ? (
          <span key={i}>
            <span className="term-ps">{cwd}&gt;</span>
            {run.text.slice(cwd.length + 1)}
          </span>
        ) : (
          run.text
        ),
      )}
    </pre>
  );
}

/** Tab id for the read-only agent section; real terminals start at 1. */
const AGENT_ID = 0;

export function TerminalPanel({
  cwd,
  agentLog,
  maxed,
  onToggleMax,
  onClose,
}: {
  cwd: string;
  /** Commands the agent ran, and their output. Read-only. */
  agentLog: string;
  maxed: boolean;
  onToggleMax: () => void;
  onClose: () => void;
}) {
  const [terms, setTerms] = useState<Term[]>([{ id: 1, name: "Terminal 1", log: "" }]);
  const [active, setActive] = useState(1);
  const [cmd, setCmd] = useState("");
  const [busy, setBusy] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const onAgent = active === AGENT_ID;
  const current = terms.find((t) => t.id === active) || terms[0];

  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [current?.log, agentLog, busy]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [active, maxed]);

  function add() {
    nextId += 1;
    const t = { id: nextId, name: nextName(terms), log: "" };
    setTerms([...terms, t]);
    setActive(t.id);
  }

  /**
   * Close one tab. Closing the last one also hides the panel and resets to a
   * single clean terminal, so reopening does not resurrect the closed one.
   */
  function closeTab(id: number) {
    const idx = terms.findIndex((t) => t.id === id);
    const next = terms.filter((t) => t.id !== id);
    if (next.length === 0) {
      nextId += 1;
      setTerms([{ id: nextId, name: "Terminal 1", log: "" }]);
      setActive(nextId);
      onClose();
      return;
    }
    setTerms(next);
    if (id === active) setActive(next[Math.min(idx, next.length - 1)].id);
  }

  function append(text: string) {
    setTerms((prev) => prev.map((t) => (t.id === active ? { ...t, log: t.log + text } : t)));
  }

  async function run() {
    const line = cmd.trim();
    if (!line || busy) return;
    setCmd("");
    setBusy(true);
    append(`${cwd}> ${line}\n`);
    try {
      const r = await api.shell(line);
      const out = `${r.stdout}${r.stderr}`;
      append(out || "");
      if (r.exit_code !== 0) append(`\nexit ${r.exit_code}\n`);
      if (out && !out.endsWith("\n")) append("\n");
    } catch (e) {
      append(`${errText(e)}\n`);
    }
    setBusy(false);
  }

  return (
    <div className="term">
      <div className="term-tabs">
        <div className={`term-tab ${onAgent ? "on" : ""}`}>
          <button className="term-tab-name" onClick={() => setActive(AGENT_ID)}>
            Agent
          </button>
        </div>
        <span className="term-sep" />
        {terms.map((t) => (
          <div key={t.id} className={`term-tab ${t.id === active ? "on" : ""}`}>
            <button className="term-tab-name" onClick={() => setActive(t.id)}>
              {t.name}
            </button>
            <button
              className="term-tab-x"
              title={terms.length > 1 ? `Close ${t.name}` : "Hide terminal"}
              onClick={() => closeTab(t.id)}
            >
              <IconClose />
            </button>
          </div>
        ))}
        <button className="icon-btn term-add" onClick={add} title="New terminal">
          <IconPlus />
        </button>
        <span className="spacer" />
        <button
          className="icon-btn"
          onClick={onToggleMax}
          title={maxed ? "Restore panel" : "Maximize panel"}
        >
          <IconMaximize />
        </button>
        <button className="icon-btn" onClick={onClose} title="Hide terminal">
          <IconClose />
        </button>
      </div>

      <div className="term-body" ref={bodyRef} onClick={() => inputRef.current?.focus()}>
        {onAgent ? (
          agentLog ? (
            <LogView log={agentLog} cwd={cwd} />
          ) : (
            <div className="panel-empty">Commands the agent runs appear here.</div>
          )
        ) : (
          <>
            {current?.log && <LogView log={current.log} cwd={cwd} />}
            <div className="term-line">
              <span className="term-ps">{cwd}&gt;</span>
              {busy ? (
                <span className="term-running">running…</span>
              ) : (
                <input
                  ref={inputRef}
                  value={cmd}
                  onChange={(e) => setCmd(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") run();
                  }}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="Terminal command"
                />
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
