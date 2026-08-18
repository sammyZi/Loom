"use client";

import { CodeEditor } from "@/components/CodeEditor";
import { DiffView } from "@/components/DiffView";
import { FileTree } from "@/components/FileTree";
import { GitPanel } from "@/components/GitPanel";
import { TerminalPanel } from "@/components/TerminalPanel";
import { Welcome, rememberRecent } from "@/components/Welcome";
import { api, type AgentEvent, type FileNode, wsBase } from "@/lib/api";
import { useCallback, useEffect, useRef, useState } from "react";

const MODEL_KEY = "ide-ai-model";
const SIDE_KEY = "ide-ai-side";
const PREVIEW_KEY = "ide-ai-preview";
const MODELS = [
  { id: "deepseek-v4-flash", label: "Flash" },
  { id: "deepseek-v4-pro", label: "Pro" },
] as const;

type LogItem = { kind: string; text: string };

export default function Page() {
  const [folder, setFolder] = useState<string | null>(null);
  const [tree, setTree] = useState<FileNode | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [tab, setTab] = useState<"preview" | "code" | "diff">("code");
  const [nav, setNav] = useState<"sessions" | "files">("files");
  const [termOpen, setTermOpen] = useState(true);
  const [sideOpen, setSideOpen] = useState(true);
  const [previewOpen, setPreviewOpen] = useState(true);
  const feedRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const [diff, setDiff] = useState("");
  const [git, setGit] = useState<{ branch: string; files: { path: string; status: string }[] } | null>(null);
  const [log, setLog] = useState<LogItem[]>([]);
  const [promptShown, setPromptShown] = useState("");
  const [busy, setBusy] = useState(false);
  const [phase, setPhase] = useState("");
  const [copied, setCopied] = useState(false);
  const [model, setModel] = useState("deepseek-v4-pro");
  const [prompt, setPrompt] = useState("");
  const [err, setErr] = useState("");

  useEffect(() => {
    const saved = localStorage.getItem(MODEL_KEY);
    if (saved) setModel(saved);
    setSideOpen(localStorage.getItem(SIDE_KEY) !== "0");
    setPreviewOpen(localStorage.getItem(PREVIEW_KEY) !== "0");
  }, []);

  // keep the feed pinned to the bottom while streaming, unless the user scrolled up
  useEffect(() => {
    const el = feedRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [log, promptShown]);

  const refresh = useCallback(async () => {
    const ws = await api.workspace();
    setFolder(ws.path);
    if (!ws.path) return;
    try {
      setTree((await api.tree()) as FileNode);
    } catch {
      setTree(null);
    }
    try {
      setGit(await api.gitStatus());
    } catch {
      setGit(null);
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
      if (ev.type === "diff") {
        setDiff(ev.diff);
        setTab("diff");
        refresh().catch(() => {});
      }
      const line = formatEvent(ev);
      if (line) setLog((prev) => mergeLog(prev, line));
      if (ev.type === "done") {
        setLog((prev) => {
          if (prev.some((l) => l.kind === "token")) return prev;
          return [...prev, { kind: "ok", text: ev.summary }];
        });
      }
    };
    return () => {
      files.close();
      agent.close();
    };
  }, [refresh]);

  async function pick() {
    setErr("");
    try {
      const r = await api.pick();
      if (r.path) {
        rememberRecent(r.path);
        await refresh();
      }
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  async function openRecent(path: string) {
    setErr("");
    try {
      await api.open(path);
      rememberRecent(path);
      await refresh();
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  async function openFile(path: string) {
    if (dirty && active) await api.save(active, content).catch(() => {});
    const f = await api.content(path);
    setActive(path);
    setContent(f.content);
    setDirty(false);
    setTab("code");
    try {
      setDiff((await api.gitDiff(path)).diff);
    } catch {
      setDiff("");
    }
  }

  async function save() {
    if (!active) return;
    await api.save(active, content);
    setDirty(false);
    refresh().catch(() => {});
  }

  async function run() {
    setErr("");
    if (!prompt.trim()) return;
    setPromptShown(prompt);
    setLog([]);
    try {
      await api.runAgent(prompt, model);
      setPrompt("");
      setBusy(true);
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    }
  }

  async function collab() {
    try {
      await navigator.clipboard.writeText(window.location.href);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  const project = folder?.split(/[\\/]/).filter(Boolean).pop() || "Project";

  if (!folder) {
    return (
      <>
        <Welcome onPick={pick} onOpenRecent={openRecent} />
        {err && <div className="welcome-err">{err}</div>}
      </>
    );
  }

  return (
    <div className="app">
      <div className="work">
        <aside className={`sidebar ${sideOpen ? "" : "off"}`}>
          <div className="brand">IDE-AI</div>
          <nav className="nav">
            <button className={nav === "sessions" ? "on" : ""} onClick={() => setNav("sessions")}>
              Sessions
            </button>
            <button className={nav === "files" ? "on" : ""} onClick={() => setNav("files")}>
              Files
            </button>
          </nav>
          {nav === "sessions" ? (
            <>
              <div className="session on">
                <div className="session-title">{promptShown || "New session"}</div>
                <div className="session-meta">
                  <span className={`dot ${busy ? "run" : ""}`} />
                  {busy ? "Running" : "Ready"} · {model.includes("flash") ? "Flash" : "Pro"}
                </div>
              </div>
              <div className="recent-label">Agents</div>
              <div className="agents">
                {["planner", "coder", "reviewer"].map((name) => {
                  const on = busy && phase.toLowerCase().startsWith(name);
                  return (
                    <div key={name} className={`agent-row ${on ? "on" : ""}`}>
                      <span className={`dot ${on ? "run" : "off"}`} />
                      {name}
                    </div>
                  );
                })}
              </div>
            </>
          ) : (
            <div className="files-pane">
              <FileTree tree={tree} active={active} onOpen={openFile} />
              <GitPanel status={git} onSelect={openFile} onRefresh={() => refresh().catch(() => {})} />
            </div>
          )}
          <div className="side-foot">
            <div className="folder-chip" title={folder}>
              {folder}
            </div>
            <button className="btn" onClick={pick} style={{ width: "100%" }}>
              Open folder
            </button>
          </div>
        </aside>

        <main className="center">
          <div className="crumb">
            <button
              className="icon-btn"
              title={sideOpen ? "Hide sidebar" : "Show sidebar"}
              onClick={() => {
                localStorage.setItem(SIDE_KEY, sideOpen ? "0" : "1");
                setSideOpen(!sideOpen);
              }}
            >
              <IconPanel />
            </button>
            <span className="crumb-label">
              {project} / <b>{promptShown || "New task"}</b>
            </span>
            <span className="spacer" />
            {dirty && (
              <button className="btn btn-primary" onClick={save}>
                Save
              </button>
            )}
            <button className="btn" onClick={collab}>
              {copied ? "Copied" : "Collaborate"}
            </button>
            <button
              className="icon-btn"
              title={previewOpen ? "Hide editor" : "Show editor"}
              onClick={() => {
                localStorage.setItem(PREVIEW_KEY, previewOpen ? "0" : "1");
                setPreviewOpen(!previewOpen);
              }}
            >
              <IconPanel flip />
            </button>
          </div>
          <div
            className="feed"
            ref={feedRef}
            onScroll={(e) => {
              const el = e.currentTarget;
              stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
            }}
          >
            {promptShown && (
              <div className="user-row">
                <div className="user-bubble">{promptShown}</div>
              </div>
            )}
            {log.map((l, i) => (
              <FeedItem key={i} item={l} />
            ))}
          </div>
          <div className="composer-wrap">
            <p className="ready">{busy ? `Working · ${phase || "agent"}` : "Ready for instructions"}</p>
            <div className="composer">
              <textarea
                value={prompt}
                onChange={(e) => setPrompt(e.target.value)}
                placeholder="Ask ide-ai to build features, fix bugs, or work on your code."
                onKeyDown={(e) => {
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) run();
                }}
              />
              <div className="composer-row">
                <div className="seg">
                  {MODELS.map((m) => (
                    <button
                      key={m.id}
                      className={model === m.id ? "on" : ""}
                      disabled={busy}
                      onClick={() => {
                        setModel(m.id);
                        localStorage.setItem(MODEL_KEY, m.id);
                      }}
                    >
                      {m.label}
                    </button>
                  ))}
                </div>
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
            {err && <div className="err">{err}</div>}
          </div>
        </main>

        <section className={`preview ${previewOpen ? "" : "off"}`}>
          <div className="preview-head">
            <span className="preview-title">{active || "Preview"}</span>
            <div className="tabs">
              <button className={tab === "preview" ? "on" : ""} onClick={() => setTab("preview")}>
                Preview
              </button>
              <button className={tab === "code" ? "on" : ""} onClick={() => setTab("code")}>
                Code
              </button>
              <button className={tab === "diff" ? "on" : ""} onClick={() => setTab("diff")}>
                Diff
              </button>
            </div>
          </div>
          <div className="preview-body">
            {tab === "diff" ? (
              <DiffView diff={diff} />
            ) : (
              <CodeEditor
                path={active}
                value={content}
                onChange={(v) => {
                  setContent(v);
                  setDirty(true);
                }}
              />
            )}
          </div>
        </section>
      </div>

      <div className={`shell-wrap ${termOpen ? "" : "off"}`}>
        <div className="shell-bar">
          <button className="btn" onClick={() => setTermOpen((v) => !v)}>
            {termOpen ? "Close terminal" : "Terminal"}
          </button>
          <span className="spacer" />
        </div>
        <TerminalPanel />
      </div>
    </div>
  );
}

function IconPanel({ flip }: { flip?: boolean }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden style={flip ? { transform: "scaleX(-1)" } : undefined}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M9 4v16" />
    </svg>
  );
}

function FeedItem({ item }: { item: LogItem }) {
  if (item.kind === "phase") {
    return (
      <div className="event">
        <span className="badge warn">{item.text}</span>
      </div>
    );
  }
  if (item.kind === "tool") {
    return (
      <div className="event">
        <span className="badge tool">{item.text}</span>
      </div>
    );
  }
  if (item.kind === "err") {
    return (
      <div className="card">
        <span className="badge err">error</span>
        <div>{item.text}</div>
      </div>
    );
  }
  if (item.kind === "ok") {
    return (
      <div className="event">
        <span className="badge ok">done</span>
        <div style={{ marginTop: 8, whiteSpace: "pre-wrap" }}>{item.text}</div>
      </div>
    );
  }
  return <div className={`event ${item.kind}`}>{item.text}</div>;
}

function formatEvent(ev: AgentEvent): LogItem | null {
  switch (ev.type) {
    case "token":
      return { kind: "token", text: ev.text };
    case "tool_call":
      return { kind: "tool", text: ev.name };
    case "done":
    case "status":
    case "think":
    case "tool_result":
    case "diff":
      return null;
    case "error":
      return { kind: "err", text: ev.message };
    default:
      return null;
  }
}

function mergeLog(prev: LogItem[], line: LogItem) {
  if (
    (line.kind === "token" || line.kind === "think") &&
    prev.length &&
    prev[prev.length - 1].kind === line.kind
  ) {
    const copy = prev.slice();
    copy[copy.length - 1] = { kind: line.kind, text: copy[copy.length - 1].text + line.text };
    return copy;
  }
  return [...prev, line];
}
