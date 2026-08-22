"use client";

import {
  IconCheck,
  IconClose,
  IconCopy,
  IconMaximize,
  IconPlay,
  IconPlus,
  IconStop,
} from "@/components/Icons";
import { api, connectWs, type AgentEvent, type ShellEvent } from "@/lib/api";
import { errText, toolLabel } from "@/lib/log";
import { useEffect, useRef, useState } from "react";

/** `job` keys this terminal's running command on the server; it only has to be
 *  unique among open terminals, including ones in another window. `running`
 *  covers both foreground commands and background (keep-alive) processes. */
type Term = { id: number; name: string; log: string; job: string; running: boolean };

let nextId = 1;

const newJob = () => `${Date.now()}-${Math.random().toString(36).slice(2)}`;

/** Backend notes appended to the stream when a background job leaves the world. */
const EXIT_NOTE = /\[exited code \d+\]|\[stopped\]/;

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
  const [terms, setTerms] = useState<Term[]>([
    { id: 1, name: "Terminal 1", log: "", job: newJob(), running: false },
  ]);
  const [active, setActive] = useState(1);
  const [cmd, setCmd] = useState("");
  const [copied, setCopied] = useState(false);
  // When set, Enter starts the command as a background process that keeps
  // running after the call returns — how dev servers stay alive here.
  const [bgMode, setBgMode] = useState(false);
  // Which terminal's job is in flight overall, so Ctrl+C still hits the one
  // that was started last even after switching tabs mid-run.
  const runningJob = useRef<string | null>(null);
  // Jobs whose output already arrived over the socket, so the final HTTP
  // response does not print the same text a second time.
  const streamed = useRef(new Set<string>());
  // Jobs the user stopped. Their non-zero exit is expected, so it is not
  // reported as a failure — ^C is the whole story, as in a real shell.
  const interrupted = useRef(new Set<string>());
  const bodyRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const onAgent = active === AGENT_ID;
  const current = terms.find((t) => t.id === active) || terms[0];
  const activeRunning = current?.running ?? false;

  const markRunning = (id: number, running: boolean) =>
    setTerms((prev) => prev.map((t) => (t.id === id ? { ...t, running } : t)));
  const markJobDone = (job: string) =>
    setTerms((prev) => prev.map((t) => (t.job === job ? { ...t, running: false } : t)));

  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [current?.log, agentLog, activeRunning]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [active, maxed]);

  // The prompt unmounts while a command runs, so focus would otherwise fall out
  // of the panel and typing would go nowhere. Park it on the body meanwhile —
  // which also keeps Ctrl+C scoped here — then hand it back when the prompt
  // returns, but only if the terminal still had it: yanking focus away from
  // wherever the user moved on to would be worse than losing it.
  useEffect(() => {
    const root = bodyRef.current;
    if (activeRunning) {
      root?.focus();
      return;
    }
    const activeEl = document.activeElement;
    if (root && (activeEl === document.body || root.contains(activeEl))) {
      inputRef.current?.focus();
    }
  }, [activeRunning]);

  // Follow output live. Without this a long command shows nothing at all until
  // it exits, which is what made `ping -t` look like a hang. connectWs keeps
  // the stream alive across backend restarts instead of dying permanently.
  useEffect(() => {
    const stop = connectWs("/ws/shell", (data) => {
      const ev = data as ShellEvent;
      if (!ev || ev.type !== "chunk") return;
      setTerms((prev) => {
        // Ignore chunks for terminals this window does not own, e.g. a second
        // window driving the same backend.
        if (!prev.some((t) => t.job === ev.id)) return prev;
        streamed.current.add(ev.id);
        return prev.map((t) => (t.job === ev.id ? { ...t, log: t.log + ev.text } : t));
      });
      // Background jobs have no follow-up response; the backend's exit note is
      // what flips their tab back to idle.
      if (EXIT_NOTE.test(ev.text)) markJobDone(ev.id);
    });
    return () => stop();
  }, []);

  function add() {
    nextId += 1;
    const t = { id: nextId, name: nextName(terms), log: "", job: newJob(), running: false };
    setTerms([...terms, t]);
    setActive(t.id);
  }

  /**
   * Close one tab. Closing the last one also hides the panel and resets to a
   * single clean terminal, so reopening does not resurrect the closed one.
   * Either way the tab's command is killed rather than left running headless.
   */
  function closeTab(id: number) {
    const idx = terms.findIndex((t) => t.id === id);
    const closing = terms[idx];
    if (closing) {
      api.cancelShell(closing.job).catch(() => {});
      if (runningJob.current === closing.job) runningJob.current = null;
    }
    const next = terms.filter((t) => t.id !== id);
    if (next.length === 0) {
      nextId += 1;
      setTerms([{ id: nextId, name: "Terminal 1", log: "", job: newJob(), running: false }]);
      setActive(nextId);
      onClose();
      return;
    }
    setTerms(next);
    if (id === active) setActive(next[Math.min(idx, next.length - 1)].id);
  }

  /** Copy the whole visible log — selecting a long scrollback by hand is worse. */
  async function copyAll() {
    const text = onAgent ? agentLog : (current?.log ?? "");
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    } catch {
      /* clipboard blocked; the selection path still works */
    }
  }

  /** Ctrl+C / the stop button: kill the running command, like a real terminal. */
  function interrupt() {
    const job = runningJob.current;
    if (!job) return;
    interrupted.current.add(job);
    api.cancelShell(job).catch(() => {});
    const tab = terms.find((t) => t.job === job);
    if (tab) append("^C\n", tab.id);
  }

  /** Defaults to the active tab, but a run must write to the tab it started in
   *  even if the user has switched away while it was still going. */
  function append(text: string, tabId = active) {
    setTerms((prev) => prev.map((t) => (t.id === tabId ? { ...t, log: t.log + text } : t)));
  }

  /** Shared prologue: claim the tab, echo the line, remember the job. */
  function beginRun(line: string, tag?: string): { tab: Term } | null {
    const tab = current;
    if (!line || !tab || tab.running) return null;
    setCmd("");
    markRunning(tab.id, true);
    runningJob.current = tab.job;
    streamed.current.delete(tab.job);
    interrupted.current.delete(tab.job);
    append(`${cwd}> ${line}${tag ? `  ${tag}` : ""}\n`, tab.id);
    return { tab };
  }

  async function run() {
    const line = cmd.trim();
    const started = beginRun(line);
    if (!started) return;
    const tab = started.tab;
    try {
      const r = await api.shell(line, tab.job);
      if ("started" in r) return; // background answered early; not a fg run
      // Already printed live by the socket; only fall back to the response body
      // when nothing streamed (socket down, or a backend without /ws/shell).
      const out = streamed.current.has(tab.job) ? "" : `${r.stdout}${r.stderr}`;
      if (out) {
        append(out, tab.id);
        if (!out.endsWith("\n")) append("\n", tab.id);
      }
      if (r.exit_code !== 0 && !interrupted.current.has(tab.job)) {
        append(`\nexit ${r.exit_code}\n`, tab.id);
      }
    } catch (e) {
      append(`${errText(e)}\n`, tab.id);
    }
    streamed.current.delete(tab.job);
    interrupted.current.delete(tab.job);
    if (runningJob.current === tab.job) runningJob.current = null;
    markRunning(tab.id, false);
  }

  /** Start a dev server or other long-running program. Returns immediately;
   *  output keeps streaming into this tab until it exits or is stopped. */
  async function runBg() {
    const line = cmd.trim();
    const started = beginRun(line, "[keep running]");
    if (!started) return;
    try {
      await api.shell(line, started.tab.job, true);
    } catch (e) {
      append(`${errText(e)}\n`, started.tab.id);
      if (runningJob.current === started.tab.job) runningJob.current = null;
      markRunning(started.tab.id, false);
    }
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
              {t.running && <span className="term-run-dot" aria-hidden />}
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
        <button className="icon-btn" onClick={copyAll} title="Copy output">
          {copied ? <IconCheck /> : <IconCopy />}
        </button>
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

      <div
        className="term-body"
        ref={bodyRef}
        tabIndex={-1}
        onClick={() => {
          // A drag to select output ends in a click. Focusing the prompt here
          // would drop that selection, which made the output impossible to copy.
          if (window.getSelection()?.toString()) return;
          if (!activeRunning) inputRef.current?.focus();
        }}
        onKeyDown={(e) => {
          const mod = e.ctrlKey || e.metaKey;
          if (mod && e.key === "v" && !onAgent && !activeRunning) {
            // Paste belongs at the prompt wherever the focus happens to be;
            // the browser's own paste then lands in the now-focused input.
            inputRef.current?.focus();
            return;
          }
          if (!mod || e.key !== "c") return;
          // With text selected, Ctrl+C still means copy — same as a real terminal.
          if (window.getSelection()?.toString()) return;
          e.preventDefault();
          interrupt();
        }}
      >
        {onAgent ? (
          agentLog ? (
            <LogView log={agentLog} cwd={cwd} />
          ) : (
            <div className="panel-empty">Commands the agent runs appear here.</div>
          )
        ) : (
          <>
            {current?.log && <LogView log={current.log} cwd={cwd} />}
            {/* One command per tab: while it runs we show a live status row with
                an explicit stop, then bring the prompt back when it settles. */}
            {!activeRunning ? (
              <div className="term-line">
                <span className="term-ps">{cwd}&gt;</span>
                <input
                  ref={inputRef}
                  value={cmd}
                  onChange={(e) => setCmd(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") (bgMode ? runBg : run)();
                  }}
                  spellCheck={false}
                  autoComplete="off"
                  aria-label="Terminal command"
                  placeholder={bgMode ? "command (runs in background)" : undefined}
                />
                <button
                  className={`icon-btn term-bg ${bgMode ? "on" : ""}`}
                  title={
                    bgMode
                      ? "Enter will KEEP THIS RUNNING — for dev servers and watchers"
                      : "Switch to keep-running mode: Enter starts the command in the background so servers stay alive"
                  }
                  aria-pressed={bgMode}
                  onClick={() => setBgMode(!bgMode)}
                >
                  <IconPlay />
                </button>
              </div>
            ) : (
              <div className="term-line">
                <span className="act-live" />
                <span className="term-idle">
                  running… Ctrl+C or the square stops it
                </span>
                <button
                  className="icon-btn term-stop"
                  title="Stop the running command"
                  onClick={interrupt}
                >
                  <IconStop />
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
