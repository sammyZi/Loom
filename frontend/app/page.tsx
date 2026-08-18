"use client";

import { AgentPanel, formatEvent } from "@/components/AgentPanel";
import { CodeEditor } from "@/components/CodeEditor";
import { DiffView } from "@/components/DiffView";
import { FileTree } from "@/components/FileTree";
import { GitPanel } from "@/components/GitPanel";
import { api, type AgentEvent, type FileNode, wsBase } from "@/lib/api";
import { useCallback, useEffect, useState } from "react";

export default function Page() {
  const [folder, setFolder] = useState<string | null>(null);
  const [tree, setTree] = useState<FileNode | null>(null);
  const [active, setActive] = useState<string | null>(null);
  const [content, setContent] = useState("");
  const [dirty, setDirty] = useState(false);
  const [tab, setTab] = useState<"code" | "diff">("code");
  const [diff, setDiff] = useState("");
  const [git, setGit] = useState<{ branch: string; files: { path: string; status: string }[] } | null>(
    null,
  );
  const [log, setLog] = useState<{ kind: string; text: string }[]>([]);
  const [busy, setBusy] = useState(false);

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
      if (ev.type === "status") setBusy(true);
      if (ev.type === "done" || ev.type === "error") setBusy(false);
      if (ev.type === "diff") {
        setDiff(ev.diff);
        setTab("diff");
        refresh().catch(() => {});
      }
      const line = formatEvent(ev);
      if (line) setLog((prev) => mergeLog(prev, line));
    };
    return () => {
      files.close();
      agent.close();
    };
  }, [refresh]);

  async function pick() {
    const r = await api.pick();
    if (r.path) {
      setFolder(r.path);
      await refresh();
    }
  }

  async function openFile(path: string) {
    if (dirty && active) {
      await api.save(active, content).catch(() => {});
    }
    const f = await api.content(path);
    setActive(path);
    setContent(f.content);
    setDirty(false);
    setTab("code");
    try {
      const d = await api.gitDiff(path);
      setDiff(d.diff);
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

  if (!folder) {
    return (
      <div className="app">
        <div className="empty">
          <h1>ide-ai</h1>
          <p>Open a folder. The agent can only use that folder.</p>
          <button onClick={pick}>Open Folder</button>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <div className="topbar">
        <button onClick={pick}>Open Folder</button>
        <span className="path">{folder}</span>
        {dirty && (
          <button onClick={save} style={{ marginLeft: "auto" }}>
            Save
          </button>
        )}
      </div>
      <div className="body">
        <div className="col">
          <div className="head">Files</div>
          <FileTree tree={tree} active={active} onOpen={openFile} />
          <GitPanel
            status={git}
            onSelect={openFile}
            onRefresh={() => refresh().catch(() => {})}
          />
        </div>
        <div className="col">
          <div className="tabs">
            <button className={tab === "code" ? "on" : ""} onClick={() => setTab("code")}>
              {active || "Editor"}
            </button>
            <button className={tab === "diff" ? "on" : ""} onClick={() => setTab("diff")}>
              Diff
            </button>
          </div>
          {tab === "code" ? (
            <CodeEditor
              path={active}
              value={content}
              onChange={(v) => {
                setContent(v);
                setDirty(true);
              }}
            />
          ) : (
            <DiffView diff={diff} />
          )}
        </div>
        <div className="col">
          <AgentPanel log={log} busy={busy} />
        </div>
      </div>
    </div>
  );
}

function mergeLog(
  prev: { kind: string; text: string }[],
  line: { kind: string; text: string },
) {
  if (line.kind === "token" && prev.length && prev[prev.length - 1].kind === "token") {
    const copy = prev.slice();
    copy[copy.length - 1] = { kind: "token", text: copy[copy.length - 1].text + line.text };
    return copy;
  }
  return [...prev, line];
}
