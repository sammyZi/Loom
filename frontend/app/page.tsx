"use client";

import { Composer, type Attachment } from "@/components/Composer";
import { ContextBar, type Git } from "@/components/ContextBar";
import { DiffPanel } from "@/components/DiffPanel";
import { Feed } from "@/components/Feed";
import { Sidebar } from "@/components/Sidebar";
import { TerminalPanel } from "@/components/TerminalPanel";
import { TopBar, type Panel } from "@/components/TopBar";
import { Welcome, rememberRecent, type Recent } from "@/components/Welcome";
import { api, type AgentEvent, wsBase } from "@/lib/api";
import { baseName, countDiff, errText, formatEvent, mergeLog, type LogItem } from "@/lib/log";
import { deleteSession, loadSessions, newSessionId, saveSession, type Session } from "@/lib/store";
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
  const [log, setLog] = useState<LogItem[]>([]);
  const [promptShown, setPromptShown] = useState("");
  const [prompt, setPrompt] = useState("");
  const [attached, setAttached] = useState<Attachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState("");
  const [elapsed, setElapsed] = useState(0);
  const [model, setModel] = useState("deepseek-v4-pro");
  const [sideOpen, setSideOpen] = useState(true);
  const [panel, setPanel] = useState<Panel>("none");
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState("");

  // latest values for the persist-on-finish effect, so it need not re-run per token
  const live = useRef({ sessionId, promptShown, log, folder });
  live.current = { sessionId, promptShown, log, folder };

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

  useEffect(() => {
    if (folder) setSessions(loadSessions(folder));
  }, [folder]);

  useEffect(() => {
    if (!busy) return;
    setElapsed(0);
    const t = setInterval(() => setElapsed((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [busy]);

  // persist the transcript once a run settles
  useEffect(() => {
    if (busy) return;
    const { sessionId: id, promptShown: t, log: items, folder: dir } = live.current;
    if (!id || !dir || items.length === 0) return;
    setSessions(saveSession({ id, folder: dir, title: t || "Untitled", log: items, at: Date.now() }));
  }, [busy]);

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

  useEffect(() => {
    const files = new WebSocket(`${wsBase()}/ws/files`);
    files.onmessage = () => {
      refresh().catch(() => {});
    };
    const agent = new WebSocket(`${wsBase()}/ws/agent`);
    agent.onmessage = (m) => {
      const ev = JSON.parse(m.data) as AgentEvent;
      if (ev.type === "status") {
        setBusy(true);
        setPhase(ev.message);
      }
      if (ev.type === "done" || ev.type === "error") {
        setBusy(false);
        setPhase("");
      }
      if (ev.type === "diff") refresh().catch(() => {});
      const line = formatEvent(ev);
      if (line) setLog((prev) => mergeLog(prev, line));
      if (ev.type === "done") {
        setLog((prev) =>
          prev.some((l) => l.kind === "token") ? prev : [...prev, { kind: "ok", text: ev.summary }],
        );
      }
    };
    return () => {
      files.close();
      agent.close();
    };
  }, [refresh]);

  function newTask() {
    setLog([]);
    setPromptShown("");
    setSessionId(null);
    setAttached([]);
  }

  async function pick() {
    setErr("");
    try {
      const r = await api.pick();
      if (!r.path) return;
      rememberRecent(r.path);
      loadRecent();
      newTask();
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
      await refresh();
    } catch (e) {
      setErr(errText(e));
    }
  }

  async function run() {
    setErr("");
    if (!prompt.trim()) return;
    const body = attached.length
      ? `${attached.map((a) => `--- ${a.name} ---\n${a.text}`).join("\n\n")}\n\n${prompt}`
      : prompt;
    setPromptShown(prompt);
    setLog([]);
    setSessionId(newSessionId());
    try {
      await api.runAgent(body, model);
      setPrompt("");
      setAttached([]);
      setBusy(true);
    } catch (e) {
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

  if (!folder) {
    return (
      <>
        <Welcome onPick={pick} onOpenRecent={openRecent} />
        {err && <div className="welcome-err">{err}</div>}
      </>
    );
  }

  const project = baseName(folder);
  const title = promptShown || "New task";

  return (
    <div className="app">
      <div className="work">
        <Sidebar
          open={sideOpen}
          folder={folder}
          recent={recent}
          sessions={sessions}
          activeId={sessionId}
          title={title}
          busy={busy}
          onToggle={toggleSide}
          onNewTask={newTask}
          onPick={pick}
          onOpenRecent={openRecent}
          onLoadSession={(s) => {
            setSessionId(s.id);
            setPromptShown(s.title);
            setLog(s.log);
            setAttached([]);
          }}
          onDeleteSession={(id) => {
            setSessions(deleteSession(id, folder));
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
            onShowSide={toggleSide}
            onPanel={setPanel}
            onCopyLink={copyLink}
          />

          <Feed prompt={promptShown} log={log} busy={busy} phase={phase} elapsed={elapsed} />

          <div className="composer-wrap">
            <ContextBar project={project} git={git} stat={stat} onCommit={commit} />
            <Composer
              value={prompt}
              model={model}
              busy={busy}
              attached={attached}
              onChange={setPrompt}
              onModel={(id) => {
                setModel(id);
                localStorage.setItem(MODEL_KEY, id);
              }}
              onAttach={(files) =>
                setAttached((prev) => [
                  ...prev.filter((p) => !files.some((f) => f.name === p.name)),
                  ...files,
                ])
              }
              onRemove={(name) => setAttached((prev) => prev.filter((a) => a.name !== name))}
              onRun={run}
              onStop={() => api.cancelAgent()}
            />
            {err && <div className="err">{err}</div>}
          </div>
        </main>

        <div className={`shell-wrap ${panel === "terminal" ? "" : "off"}`}>
          <TerminalPanel onClose={() => setPanel("none")} />
        </div>
        <div className={`shell-wrap ${panel === "diff" ? "" : "off"}`}>
          <DiffPanel diff={diff} onRefresh={() => refresh().catch(() => {})} />
        </div>
      </div>
    </div>
  );
}
