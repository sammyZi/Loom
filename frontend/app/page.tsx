"use client";

import { Composer, MODEL_IDS, type SubmitMeta } from "@/components/Composer";
import { ContextBar, type Git } from "@/components/ContextBar";
import { DiffPanel } from "@/components/DiffPanel";
import { Feed } from "@/components/Feed";
import { Sidebar, buildGroups, normPath } from "@/components/Sidebar";
import { TerminalPanel, appendAgentLog } from "@/components/TerminalPanel";
import { TopBar, type Panel } from "@/components/TopBar";
import { Welcome, rememberRecent, type Recent } from "@/components/Welcome";
import { api, connectWs, loadArchived, type AgentEvent, type SessionLite, unarchiveSession } from "@/lib/api";
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
const SIDE_W_KEY = "ide-ai-side-w";
const PANEL_W_KEY = "ide-ai-panel-w";

/** Panel sizes with sane limits: [default, min, max]. */
const SIDE_W = { def: 260, min: 200, max: 460 };
const PANEL_W = { def: 460, min: 300, max: 900 };

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
  // Approximate context-window usage for the run in progress (and its last
  // known value afterwards), driven by backend `context` events.
  const [ctx, setCtx] = useState<{ used: number; limit: number } | null>(null);
  // Open approval request from manual mode's shell gate.
  const [pendingAsk, setPendingAsk] = useState<{ id: string; program: string; args: string } | null>(null);
  // Archived chats view in the sidebar.
  const [archiveOpen, setArchiveOpen] = useState(false);
  const [archived, setArchived] = useState<SessionLite[]>([]);
  // Resizable columns (sidebar width / terminal & diff panel width).
  const [sideW, setSideW] = useState(SIDE_W.def);
  const [panelW, setPanelW] = useState(PANEL_W.def);
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
    const sw = Number(localStorage.getItem(SIDE_W_KEY));
    if (sw >= SIDE_W.min && sw <= SIDE_W.max) setSideW(sw);
    const pw = Number(localStorage.getItem(PANEL_W_KEY));
    if (pw >= PANEL_W.min && pw <= PANEL_W.max) setPanelW(pw);
    loadRecent();
  }, []);

  /** Shared drag logic for the column-resize handles. Sizing is clamped and
   *  remembered, so a bad value can never wedge the layout. */
  function startDrag(kind: "side" | "panel") {
    return (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault();
      const startX = e.clientX;
      const startSide = sideW;
      const startPanel = panelW;
      document.body.classList.add("dragging-col");
      const move = (ev: PointerEvent) => {
        if (kind === "side") {
          const w = Math.min(SIDE_W.max, Math.max(SIDE_W.min, startSide + ev.clientX - startX));
          setSideW(w);
        } else {
          const w = Math.min(PANEL_W.max, Math.max(PANEL_W.min, startPanel - (ev.clientX - startX)));
          setPanelW(w);
        }
      };
      const up = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", up);
        document.body.classList.remove("dragging-col");
        setSideW((w) => {
          localStorage.setItem(SIDE_W_KEY, String(w));
          return w;
        });
        setPanelW((w) => {
          localStorage.setItem(PANEL_W_KEY, String(w));
          return w;
        });
      };
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", up);
    };
  }

  async function toggleArchiveView() {
    const next = !archiveOpen;
    setArchiveOpen(next);
    if (next && archived.length === 0) {
      setArchived(await loadArchived());
    }
  }

  async function doUnarchive(id: string) {
    await unarchiveSession(id).catch(() => {});
    setArchived((prev) => prev.filter((s) => s.id !== id));
    reloadSessions();
  }

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
    if (ev.type === "ask") {
      setPendingAsk({ id: ev.id, program: ev.program, args: ev.args });
    }
    if (ev.type === "context") {
      setCtx({ used: ev.used, limit: ev.limit });
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

  /**
   * Reset the whole chat surface for a fresh task or a freshly opened folder.
   * Everything project-scoped must go here: the terminal's Agent tab used to
   * keep the previous project's commands after a switch, which read like one
   * project's sessions bleeding into another.
   */
  function newTask() {
    setLog([]);
    setPromptShown("");
    setSessionId(null);
    setSessionFolder(null);
    setAgentLog("");
    setTokens(0);
    exact.current = 0;
    streamed.current = 0;
    setCtx(null);
    setPhase("");
    setErr("");
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

  async function openRecent(path: string, sessionId?: string) {
    setErr("");
    try {
      const wasOpen = normPath(path) === normPath(folder ?? "");
      await api.open(path);
      if (!wasOpen) newTask();
      rememberRecent(path);
      loadRecent();
      setShowPicker(false);
      await refresh();
      // Clicking a session of another project opens that folder and then
      // resumes the clicked transcript instead of dropping into a blank task.
      if (sessionId) {
        const all = await loadAllSessions();
        const s = all.find((x) => x.id === sessionId);
        if (s && s.folder === path) loadSession(s);
      }
    } catch (e) {
      setErr(errText(e));
    }
  }

  /** Load a stored transcript into the chat. Guarded against mid-run swaps:
   *  detaching from a live run used to merge two transcripts into one log. */
  function loadSession(s: Session) {
    if (busy) return;
    if (normPath(s.folder) !== normPath(folder ?? "")) {
      void openRecent(s.folder, s.id);
      return;
    }
    setSessionId(s.id);
    setSessionFolder(s.folder);
    setPromptShown(s.title);
    setLog(s.log);
    setTokens(0);
    exact.current = 0;
    streamed.current = 0;
    setCtx(null);
  }

  async function decideAsk(allow: boolean) {
    const ask = pendingAsk;
    if (!ask) return;
    setPendingAsk(null);
    try {
      await api.answerPermission(ask.id, allow);
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
    // The id also keys the backend's chat memory, so follow-ups keep context.
    let sid = sessionId;
    if (!sid) {
      sid = newSessionId();
      setSessionId(sid);
      setSessionFolder(folder);
    }
    // The session title stays the first message, so the sidebar name is stable.
    if (!promptShown) setPromptShown(text);
    setLog((prev) => [...prev, { kind: "user", text }]);
    exact.current = 0;
    streamed.current = 0;
    setTokens(0);
    setCtx(null);
    myRun.current = true;
    // Set busy BEFORE the request. Setting it after the await raced the websocket:
    // a fast run could emit status+done first, and the late setBusy(true) re-latched
    // it forever, so the run never "finished" and the transcript was never saved.
    setBusy(true);
    try {
      await api.runAgent(body, id, mode, effort, sid);
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
      <div
        className="work"
        style={{ ["--side-w" as string]: `${sideW}px`, ["--panel-w" as string]: `${panelW}px` }}
      >
        <Sidebar
          open={sideOpen}
          folder={folder}
          recent={recent}
          groups={buildGroups(folder, recent, sessions)}
          activeId={sessionId}
          title={title}
          busy={busy}
          archiveOpen={archiveOpen}
          archived={archived}
          onToggleArchiveView={toggleArchiveView}
          onUnarchive={doUnarchive}
          onLoadArchivedSession={(s) => {
            const conv: Session = {
              id: s.id,
              folder: s.folder,
              title: s.title,
              at: s.at,
              created: s.created ?? s.at,
              log: (Array.isArray(s.log) ? s.log : []) as LogItem[],
            };
            loadSession(conv);
          }}
          onToggle={toggleSide}
          onNewTask={newTask}
          onPick={() => setShowPicker(true)}
          onOpenRecent={openRecent}
          onLoadSession={loadSession}
          onClearAll={async () => {
            await clearAllSessions();
            setSessions([]);
            setArchived([]);
            setRecent([]);
            newTask();
            setShowPicker(true);
          }}
          onDeleteSession={async (id) => {
            await deleteSession(id);
            await reloadSessions();
            setArchived((prev) => prev.filter((s) => s.id !== id));
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
            setArchived(await loadArchived());
            if (id === sessionId) newTask();
          }}
        />
        {sideOpen && (
          <div className="gutter gutter-side" onPointerDown={startDrag("side")} title="Drag to resize the sidebar">
            <span />
          </div>
        )}

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
            ctx={ctx}
            pending={pendingAsk}
            onDecide={decideAsk}
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

        {panel !== "none" && (
          <div
            className="gutter gutter-panel"
            onPointerDown={startDrag("panel")}
            title="Drag to resize the panel"
          >
            <span />
          </div>
        )}
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
