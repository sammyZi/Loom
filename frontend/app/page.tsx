"use client";

import { Composer, MODEL_IDS, type SubmitMeta } from "@/components/Composer";
import { ContextBar, type Git } from "@/components/ContextBar";
import { DiffPanel } from "@/components/DiffPanel";
import { Feed } from "@/components/Feed";
import { Sidebar, buildGroups } from "@/components/Sidebar";
import { TerminalPanel, appendAgentLog } from "@/components/TerminalPanel";
import { TopBar, type Panel } from "@/components/TopBar";
import { Welcome, rememberRecent, type Recent } from "@/components/Welcome";
import { api, connectWs, type AgentEvent } from "@/lib/api";
import { baseName, countDiff, errText, formatEvent, mergeLog, type LogItem } from "@/lib/log";
import {
  archiveSession,
  clearAllSessions,
  deleteSession,
  loadAllSessions,
  newSessionId,
  renameSession,
  saveSession,
  type Session,
} from "@/lib/store";
import { useCallback, useEffect, useRef, useState } from "react";

const MODEL_KEY = "ide-ai-model";
const SIDE_KEY = "ide-ai-side";
const RECENT_KEY = "ide-ai-recent";

export default function Page() {
  const [folder, setFolder] = useState<string | null>(null);
  const [git, setGit] = useState<Git | null>(null);
  const [diff, setDiff] = useState("");
  const [stat, setStat] = useState({ add: 0, del: 0 });
  const [recent, setRecent] = useState<Recent[]>([]);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionId, setSessionId] = useState<string | null>(null);
  // The folder a session belongs to, captured when it starts. Saving against the
  // *current* workspace moved transcripts between projects whenever the folder
  // changed while a run was still in flight.
  const [sessionFolder, setSessionFolder] = useState<string | null>(null);
  const [log, setLog] = useState<LogItem[]>([]);
  const [promptShown, setPromptShown] = useState("");
  const [prompt, setPrompt] = useState("");
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState("");
  const [elapsed, setElapsed] = useState(0);
  const [tokens, setTokens] = useState(0);
  const [model, setModel] = useState("deepseek-v4-pro");
  const [sideOpen, setSideOpen] = useState(true);
  const [panel, setPanel] = useState<Panel>("none");
  // Shell commands the agent ran, mirrored into the terminal panel's Agent tab.
  const [agentLog, setAgentLog] = useState("");
  const [maxed, setMaxed] = useState(false);
  const [copied, setCopied] = useState(false);
  // Lets the open-folder screen be shown on demand, not only when nothing is open.
  const [showPicker, setShowPicker] = useState(false);
  const [err, setErr] = useState("");
  // Backend socket connectivity, shown as a status dot in the top bar.
  const [connected, setConnected] = useState(true);

  // latest values for the persist-on-finish effect, so it need not re-run per token
  // token accounting: exact totals from the provider, plus a live estimate
  // The agent socket is broadcast to every connected client, so a run started in
  // another window (or by a script hitting the API) used to drive this feed and
  // flip it to "working" with no prompt from the user. Only follow our own run.
  const myRun = useRef(false);
  // Read inside the agent socket handler, which must not re-subscribe when the
  // workspace changes — reconnecting mid-run would drop the rest of the stream.
  const cwdRef = useRef("");
  const exact = useRef(0);
  const streamed = useRef(0);
  const live = useRef({ sessionId, promptShown, log, sessionFolder });
  live.current = { sessionId, promptShown, log, sessionFolder };
  cwdRef.current = folder ?? "";

  useEffect(() => {
    const saved = localStorage.getItem(MODEL_KEY);
    if (saved) setModel(saved);
    setSideOpen(localStorage.getItem(SIDE_KEY) !== "0");
    loadRecent();
  }, []);

  function loadRecent() {
    try {
      setRecent(JSON.parse(localStorage.getItem(RECENT_KEY) || "[]"));
    } catch {
      setRecent([]);
    }
  }

  const reloadSessions = useCallback(async () => {
    setSessions(await loadAllSessions());
  }, []);

  useEffect(() => {
    if (folder) reloadSessions();
  }, [folder, reloadSessions]);

  useEffect(() => {
    if (!busy) return;
    setElapsed(0);
    const t = setInterval(() => setElapsed((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [busy]);

  // persist the transcript once a run settles
  useEffect(() => {
    if (busy) return;
    const { sessionId: id, promptShown: t, log: items, sessionFolder: dir } = live.current;
    if (!id || !dir || items.length === 0) return;
    saveSession({ id, folder: dir, title: t || "Untitled", log: items, at: Date.now() })
      .then(reloadSessions)
      .catch(() => {});
  }, [busy, reloadSessions]);

  const refresh = useCallback(async () => {
    const ws = await api.workspace();
    setFolder(ws.path);
    if (!ws.path) return;
    try {
      setGit(await api.gitStatus());
      const d = (await api.gitDiff()).diff;
      setDiff(d);
      setStat(countDiff(d));
    } catch {
      setGit(null);
      setDiff("");
      setStat({ add: 0, del: 0 });
    }
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  // File events arrive in bursts during builds/checks; each one used to fire a
  // full workspace+gitStatus+gitDiff refresh immediately. Coalesce them.
  const refreshTimer = useRef<number | null>(null);
  const scheduleRefresh = useCallback(() => {
    if (refreshTimer.current !== null) return;
    refreshTimer.current = window.setTimeout(() => {
      refreshTimer.current = null;
      refresh().catch(() => {});
    }, 300);
  }, [refresh]);
  useEffect(
    () => () => {
      if (refreshTimer.current !== null) clearTimeout(refreshTimer.current);
    },
    [],
  );

  useEffect(() => {
    // connectWs reconnects with backoff and drops malformed frames; both
    // sockets died permanently before when the backend restarted.
    const stopFiles = connectWs("/ws/files", scheduleRefresh, setConnected);
    const stopAgent = connectWs("/ws/agent", (data) => {
      if (typeof data !== "object" || data === null || !("type" in data)) return;
      onAgentEvent(data as AgentEvent);
    });
    return () => {
      stopFiles();
      stopAgent();
    };
  }, [scheduleRefresh]);

  function onAgentEvent(ev: AgentEvent) {
    if (!myRun.current) return;
    if (ev.type === "status") {
      setBusy(true);
      setPhase(ev.message);
    }
    if (ev.type === "done" || ev.type === "error") {
      myRun.current = false;
      setBusy(false);
      setPhase("");
    }
    if (ev.type === "usage") {
      exact.current += ev.tokens;
      streamed.current = 0;
      setTokens(exact.current);
    }
    if (ev.type === "token" || ev.type === "think") {
      streamed.current += ev.text.length;
      setTokens(exact.current + Math.round(streamed.current / 4));
    }
    if (ev.type === "diff") scheduleRefresh();
    // Mirror the agent's shell work into the terminal panel's Agent tab.
    setAgentLog((prev) => appendAgentLog(prev, ev, cwdRef.current));
    const line = formatEvent(ev);
    if (line) setLog((prev) => mergeLog(prev, line));
    if (ev.type === "done") {
      setLog((prev) =>
        prev.some((l) => l.kind === "token") ? prev : [...prev, { kind: "ok", text: ev.summary }],
      );
    }
  }

  function newTask() {
    setLog([]);
    setPromptShown("");
    setSessionId(null);
    setSessionFolder(null);
  }

  async function pick() {
    setErr("");
    try {
      const r = await api.pick();
      if (!r.path) return;
      rememberRecent(r.path);
      loadRecent();
      newTask();
      setShowPicker(false);
      await refresh();
    } catch (e) {
      setErr(errText(e));
    }
  }

  async function openRecent(path: string) {
    setErr("");
    try {
      await api.open(path);
      rememberRecent(path);
      loadRecent();
      newTask();
      setShowPicker(false);
      await refresh();
    } catch (e) {
      setErr(errText(e));
    }
  }

  async function run(text: string, meta: SubmitMeta) {
    setErr("");
    if (!text.trim() || busy) return;

    const id = MODEL_IDS[meta.model] ?? "deepseek-v4-pro";
    setModel(id);
    localStorage.setItem(MODEL_KEY, id);

    // The mode is a real request field, never a prompt preamble: prefixing the
    // prompt defeated the backend's "is this just a greeting?" check and sent
    // plain hellos through the whole planner/coder pipeline.
    const mode = meta.mode || "Auto";
    const effort = (meta.effort || "Medium").toLowerCase();
    const names = meta.attachments.map((f) => f.name).join(", ");
    const body = names ? [`Images attached in the UI: ${names}`, "", text].join("\n") : text;

    // Continue the open session instead of starting a new one on every send.
    // A fresh id is minted only when nothing is open (New task, or first message).
    if (!sessionId) {
      setSessionId(newSessionId());
      setSessionFolder(folder);
    }
    // The session title stays the first message, so the sidebar name is stable.
    if (!promptShown) setPromptShown(text);
    setLog((prev) => [...prev, { kind: "user", text }]);
    exact.current = 0;
    streamed.current = 0;
    setTokens(0);
    myRun.current = true;
    // Set busy BEFORE the request. Setting it after the await raced the websocket:
    // a fast run could emit status+done first, and the late setBusy(true) re-latched
    // it forever, so the run never "finished" and the transcript was never saved.
    setBusy(true);
    try {
      await api.runAgent(body, id, mode, effort);
      setPrompt("");
    } catch (e) {
      myRun.current = false;
      setBusy(false);
      setErr(errText(e));
    }
  }

  async function commit(message: string) {
    setErr("");
    try {
      await api.commit(message);
      await refresh();
    } catch (e) {
      setErr(errText(e));
    }
  }

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  function toggleSide() {
    localStorage.setItem(SIDE_KEY, sideOpen ? "0" : "1");
    setSideOpen(!sideOpen);
  }

  if (!folder || showPicker) {
    return (
      <>
        <Welcome
          onPick={pick}
          onOpenRecent={openRecent}
          onCancel={folder ? () => setShowPicker(false) : undefined}
        />
        {err && <div className="welcome-err">{err}</div>}
      </>
    );
  }

  const project = baseName(folder);
  const title = promptShown || "New task";
  // PromptInput keeps its own model state and defaults to models[0],
  // so put the remembered model first.
  const models = model.includes("flash") ? ["Flash", "Pro"] : ["Pro", "Flash"];

  return (
    <div className="app">
      <div className="work">
        <Sidebar
          open={sideOpen}
          folder={folder}
          recent={recent}
          groups={buildGroups(folder, recent, sessions)}
          activeId={sessionId}
          title={title}
          busy={busy}
          onToggle={toggleSide}
          onNewTask={newTask}
          onPick={() => setShowPicker(true)}
          onOpenRecent={openRecent}
          onLoadSession={(s) => {
            setSessionId(s.id);
            setSessionFolder(s.folder);
            setPromptShown(s.title);
            setLog(s.log);
          }}
          onClearAll={async () => {
            await clearAllSessions();
            setSessions([]);
            setRecent([]);
            newTask();
            setShowPicker(true);
          }}
          onDeleteSession={async (id) => {
            await deleteSession(id);
            await reloadSessions();
            if (id === sessionId) newTask();
          }}
          onRenameSession={async (id, next) => {
            await renameSession(id, next);
            await reloadSessions();
            if (id === sessionId) setPromptShown(next);
          }}
          onArchiveSession={async (id) => {
            await archiveSession(id);
            await reloadSessions();
            if (id === sessionId) newTask();
          }}
        />

        <main className="center">
          <TopBar
            title={title}
            project={project}
            sideOpen={sideOpen}
            panel={panel}
            copied={copied}
            live={connected}
            onShowSide={toggleSide}
            onPanel={setPanel}
            onCopyLink={copyLink}
          />

          <Feed
            prompt={promptShown}
            log={log}
            busy={busy}
            phase={phase}
            elapsed={elapsed}
            tokens={tokens}
          />

          <div className="composer-wrap">
            <ContextBar project={project} git={git} stat={stat} onCommit={commit} />
            <Composer
              value={prompt}
              models={models}
              busy={busy}
              onChange={setPrompt}
              onSubmit={run}
              onStop={() => api.cancelAgent()}
            />
            {err && <div className="err">{err}</div>}
          </div>
        </main>

        <div className={`shell-wrap ${panel === "terminal" ? "" : "off"} ${maxed ? "maxed" : ""}`}>
          <TerminalPanel
            cwd={folder}
            agentLog={agentLog}
            maxed={maxed}
            onToggleMax={() => setMaxed((v) => !v)}
            onClose={() => {
              setPanel("none");
              setMaxed(false);
            }}
          />
        </div>
        <div className={`shell-wrap ${panel === "diff" ? "" : "off"}`}>
          <DiffPanel
            diff={diff}
            onRefresh={() => refresh().catch(() => {})}
            onClose={() => setPanel("none")}
          />
        </div>
      </div>
    </div>
  );
}
