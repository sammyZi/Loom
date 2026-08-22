"use client";

import { IconMark } from "@/components/Icons";
import { CopyButton, Markdown, SpeakButton } from "@/components/Markdown";
import { groupLog, secs, type LogItem } from "@/lib/log";
import { useEffect, useRef } from "react";

export function Feed({
  prompt,
  log,
  busy,
  phase,
  elapsed,
  tokens,
  pending,
  onDecide,
}: {
  prompt: string;
  log: LogItem[];
  busy: boolean;
  phase: string;
  elapsed: number;
  tokens: number;
  /** Open approval request (manual mode): the agent wants to run a command. */
  pending?: { id: string; program: string; args: string } | null;
  onDecide?: (allow: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  // stay pinned to the bottom while streaming, unless the user scrolled up
  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [log, prompt, busy]);

  const groups = groupLog(log);
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
          </div>
        )}

        {groups.map((g, i) =>
          "items" in g ? (
            <ToolLine key={i} items={g.items} running={busy && i === groups.length - 1} />
          ) : (
            <Message key={i} item={g} live={busy && i === groups.length - 1} />
          ),
        )}

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

        {busy && !pending && (
          <p className="working">
            <span className="work-mark">
              <IconMark />
            </span>
            <span className="work-stats">
              {secs(elapsed)}
              {tokens > 0 && ` · ${tokens.toLocaleString()} tokens`}
              {" · "}
              <span className="act-now">still thinking…</span>
            </span>
          </p>
        )}
      </div>
    </div>
  );
}

function Message({ item, live }: { item: LogItem; live: boolean }) {
  if (item.kind === "user") {
    return (
      <div className="user-row">
        <div className="user-bubble">{item.text}</div>
      </div>
    );
  }
  if (item.kind === "think") {
    return (
      <details className="work-group think-group">
        <summary>
          {live && <span className="act-live" />}
          <span className={live ? "act-now" : ""}>Thinking</span>
        </summary>
        <div className="work-items">
          <span className="act-think">{item.text}</span>
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
          </div>
        )}
      </div>
    );
  }
  return <div className={`event ${item.kind}`}>{item.text}</div>;
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
