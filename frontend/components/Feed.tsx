"use client";

import { IconMark, IconUndo } from "@/components/Icons";
import { CopyButton, Markdown, SpeakButton } from "@/components/Markdown";
import type { TodoItem } from "@/lib/api";
import { groupLog, secs, type LogItem } from "@/lib/log";
import { useEffect, useRef, useState } from "react";

export function Feed({
  prompt,
  log,
  busy,
  phase,
  elapsed,
  tokens,
  todos = [],
  pending,
  onDecide,
  onRevert,
  onRevertTurn,
}: {
  prompt: string;
  log: LogItem[];
  busy: boolean;
  phase: string;
  elapsed: number;
  tokens: number;
  /** The agent's task list for a multi-step job. */
  todos?: TodoItem[];
  /** Open approval request (manual mode): the agent wants to run a command. */
  pending?: { id: string; program: string; args: string } | null;
  onDecide?: (allow: boolean) => void;
  /** Undo the files a reply changed, back to how this run found them. */
  onRevert?: (item: LogItem) => void;
  /** Undo every file the reply to one sent message touched. */
  onRevertTurn?: (item: LogItem) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  // stay pinned to the bottom while streaming, unless the user scrolled up
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [log, prompt, busy]);

  const groups = groupLog(log);
  // groupLog leaves one think group per turn. While the run is live the status
  // line owns *this turn's* block and the flow skips it; when it settles the
  // flow keeps it as the record. Either way it is on screen once — the index is
  // what ties the two together, so they can never both decide to show it.
  // It must be the block after the last user message: matching the first one in
  // the session hid an old turn's reasoning and drew this turn's a second time.
  const lastUser = groups.map((g) => ("items" in g ? "" : g.kind)).lastIndexOf("user");
  const thinkAt = groups.findIndex(
    (g, i) => i > lastUser && !("items" in g) && g.kind === "think",
  );
  const thinkGroup = thinkAt >= 0 ? (groups[thinkAt] as LogItem) : null;
  const liveThink = busy && thinkGroup ? thinkGroup.text : "";
  return (
    <div
      className="feed"
      ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
      }}
    >
      <div className="feed-col">
        {!prompt && log.length === 0 && (
          <div className="feed-empty">
            <IconMark />
            <p>Ask Loom to build a feature, fix a bug, or explain the code.</p>
          </div>
        )}

        {/* Sessions saved before turns were logged keep their prompt outside the log. */}
        {prompt && !log.some((l) => l.kind === "user") && (
          <div className="user-row">
            <div className="user-bubble">{prompt}</div>
            <div className="user-actions">
              <CopyButton text={prompt} />
            </div>
          </div>
        )}

        {groups.map((g, i) => {
          const last = i === groups.length - 1;
          // Skipped only while the status line is showing it.
          if (busy && i === thinkAt) return null;
          return "items" in g ? (
            <ToolLine key={i} items={g.items} running={busy && last} />
          ) : (
            <Message
              key={i}
              item={g}
              live={busy && last}
              onRevert={onRevert}
              onRevertTurn={onRevertTurn}
              canUndo={g.kind === "user" && turnUndo(log, g).count > 0}
              undoTitle={
                g.kind === "user"
                  ? `Undo this message: put ${turnUndo(log, g).count} file(s) back to how they were before it`
                  : undefined
              }
            />
          );
        })}

        {/* Approval gate (manual mode): nothing runs until the user picks. */}
        {pending && (
          <div className="perm-card" role="alertdialog" aria-label="Approve command">
            <div className="perm-head">
              <span className="badge warn">approval needed</span>
              <span className="perm-sub">The agent wants to run a command</span>
            </div>
            <code className="perm-cmd">
              {pending.program}
              {pending.args ? ` ${pending.args}` : ""}
            </code>
            <div className="perm-actions">
              <button className="perm-allow" onClick={() => onDecide?.(true)}>
                Allow once
              </button>
              <button className="perm-deny" onClick={() => onDecide?.(false)}>
                Deny
              </button>
            </div>
          </div>
        )}

        {/* The plan, above the status line: on a long job this is the only
            thing that says how much is left. */}
        {todos.length > 0 && <TodoList todos={todos} />}

        {busy && !pending && (
          // Expandable in place: "still thinking…" is where the eye already is
          // when you want to know what it is thinking, so the reasoning opens
          // here rather than sending you hunting up the transcript.
          <details className="working-wrap">
            <summary className="working">
              <span className="work-mark">
                <IconMark />
              </span>
              <span className="work-stats">
                {secs(elapsed)}
                {tokens > 0 && ` · ${tokens.toLocaleString()} tokens`}
                {" · "}
                <span className="act-now">still thinking…</span>
              </span>
              <span className="working-toggle">{liveThink ? "show" : ""}</span>
            </summary>
            <pre className="act-think working-think">
              {liveThink || "No reasoning streamed yet — some models only send their answer."}
            </pre>
          </details>
        )}
      </div>
    </div>
  );
}

/** The agent's plan, with progress. Green marks completion because that is the
 *  one thing worth reading at a glance on a long run. */
function TodoList({ todos }: { todos: TodoItem[] }) {
  const done = todos.filter((t) => t.status === "done").length;
  const pct = Math.round((done / todos.length) * 100);
  const finished = done === todos.length;
  return (
    <div className={`todos ${finished ? "all-done" : ""}`}>
      <div className="todos-head">
        <span className="todos-title">Plan</span>
        <span className="todos-count">
          {done}/{todos.length}
        </span>
        <span className="todos-bar" aria-hidden>
          <span style={{ width: `${pct}%` }} />
        </span>
      </div>
      <ol className="todos-list">
        {todos.map((t, i) => (
          <li key={i} className={`todo ${t.status}`}>
            <span className="todo-mark" aria-hidden />
            <span className="todo-text">{t.text}</span>
          </li>
        ))}
      </ol>
    </div>
  );
}

function Message({
  item,
  live,
  onRevert,
  onRevertTurn,
  canUndo = false,
  undoTitle,
}: {
  item: LogItem;
  live: boolean;
  onRevert?: (item: LogItem) => void;
  /** Undo every file the reply to this message touched. */
  onRevertTurn?: (item: LogItem) => void;
  canUndo?: boolean;
  undoTitle?: string;
}) {
  if (item.kind === "user") {
    return (
      <div className="user-row">
        <div className="user-bubble">
          {item.images && item.images.length > 0 && (
            <div className="user-images">
              {item.images.map((src, i) => (
                // eslint-disable-next-line @next/next/no-img-element
                <img
                  key={i}
                  src={src}
                  alt=""
                  className="user-image"
                  onClick={() => window.open(src, "_blank")}
                />
              ))}
            </div>
          )}
          {item.text}
        </div>
        <div className="user-actions">
          <CopyButton text={item.text} />
          {onRevertTurn && canUndo && <UndoButton onConfirm={() => onRevertTurn(item)} label="Undo this" title={undoTitle} />}
        </div>
      </div>
    );
  }
  if (item.kind === "think") {
    // Collapsed to a single short row: the reasoning is working-out, not an
    // answer. A preview of it used to sit here and ran off the right edge, so
    // the text now appears only once the row is opened.
    const lines = item.text.split("\n").filter((l) => l.trim());
    return (
      <details className="work-group think-group">
        <summary>
          {live && <span className="act-live" />}
          <span className={live ? "act-now" : ""}>Thinking</span>
          <span className="think-count">
            {lines.length} line{lines.length === 1 ? "" : "s"}
          </span>
        </summary>
        <div className="work-items">
          <pre className="act-think">{item.text}</pre>
        </div>
      </details>
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
  // The planner answers non-coding asks with a NO_CODE: marker the backend strips
  // from its summary, but the raw tokens stream here first, so strip it on the way in.
  // Agent replies are markdown; only the raw streaming caret needs plain text.
  if (item.kind === "token" || item.kind === "ok") {
    return (
      <div className={`event ${item.kind} ${live ? "live" : ""}`}>
        <Markdown text={item.text} />
        {!live && (
          <div className="event-actions">
            <CopyButton text={item.text} />
            <SpeakButton text={item.text} />
            {item.revert && Object.keys(item.revert).length > 0 && (
              <UndoButton
                onConfirm={() => onRevert?.(item)}
                done={item.reverted}
                title={`Undo: put ${Object.keys(item.revert).length} file(s) back to before this reply`}
              />
            )}
          </div>
        )}
      </div>
    );
  }
  return <div className={`event ${item.kind}`}>{item.text}</div>;
}

/**
 * The files one turn touched: every reply between this sent message and the
 * next one, minus anything already put back. One message, one undo.
 */
export function turnUndo(
  log: LogItem[],
  user: LogItem,
): { files: Record<string, string | null>; count: number } {
  const start = log.indexOf(user);
  // null is "this file did not exist before the turn" — undo deletes it.
  const files: Record<string, string | null> = {};
  if (start < 0) return { files, count: 0 };
  for (let i = start + 1; i < log.length; i++) {
    const it = log[i];
    if (it.kind === "user") break;
    if (it.reverted || !it.revert) continue;
    // Earliest snapshot wins: undoing a turn means going back to before it,
    // not to some midpoint between two replies inside it.
    for (const [path, before] of Object.entries(it.revert)) {
      if (!(path in files)) files[path] = before;
    }
  }
  return { files, count: Object.keys(files).length };
}

/**
 * Undo is destructive — it rewrites files on disk — so it asks first, using the
 * same click-again pattern as "Clear all sessions" rather than a modal. The arm
 * lapses after a few seconds so a stray click cannot sit there waiting to fire.
 */
function UndoButton({
  onConfirm,
  done = false,
  title,
  label = "Undo",
}: {
  onConfirm: () => void;
  done?: boolean;
  title?: string;
  label?: string;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = setTimeout(() => setArmed(false), 4000);
    return () => clearTimeout(t);
  }, [armed]);

  if (done) {
    return (
      <button className="copy-btn ok" disabled title="Already undone">
        <IconUndo />
        <span>Undone</span>
      </button>
    );
  }
  return (
    <button
      className={`copy-btn ${armed ? "warn" : ""}`}
      onClick={() => (armed ? (setArmed(false), onConfirm()) : setArmed(true))}
      title={armed ? "Click again to restore those files" : title}
    >
      <IconUndo />
      <span>{armed ? "Click again to undo" : label}</span>
    </button>
  );
}

function ToolLine({ items, running }: { items: LogItem[]; running: boolean }) {
  const latest = items[items.length - 1];
  return (
    <details className="work-group">
      <summary>
        {running ? (
          <>
            <span className="act-live" />
            <span className="act-now">
              {latest.text}
              {latest.detail ? ` ${latest.detail}` : ""}
            </span>
          </>
        ) : (
          `Ran ${items.length} command${items.length === 1 ? "" : "s"}`
        )}
      </summary>
      <div className="work-items">
        {items.map((t, i) => (
          <span className="act" key={i}>
            <span className="act-verb">{t.text}</span>
            {t.detail && <code className="act-target">{t.detail}</code>}
          </span>
        ))}
      </div>
    </details>
  );
}
