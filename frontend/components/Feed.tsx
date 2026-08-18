"use client";

import { IconMark } from "@/components/Icons";
import { groupLog, mmss, type LogItem } from "@/lib/log";
import { useEffect, useRef } from "react";

export function Feed({
  prompt,
  log,
  busy,
  phase,
  elapsed,
}: {
  prompt: string;
  log: LogItem[];
  busy: boolean;
  phase: string;
  elapsed: number;
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
            <p>Ask ide-ai to build a feature, fix a bug, or explain the code.</p>
          </div>
        )}

        {prompt && (
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

        {busy && (
          <p className="working">
            <span className="spinner" />
            {phase || "Working"} · {mmss(elapsed)}
          </p>
        )}
      </div>
    </div>
  );
}

function Message({ item, live }: { item: LogItem; live: boolean }) {
  if (item.kind === "err") {
    return (
      <div className="card">
        <span className="badge err">error</span>
        <div>{item.text}</div>
      </div>
    );
  }
  return <div className={`event ${item.kind} ${live ? "live" : ""}`}>{item.text}</div>;
}

function ToolLine({ items, running }: { items: string[]; running: boolean }) {
  return (
    <details className="work-group">
      <summary>
        {running ? "Running" : "Ran"} {items.length} command{items.length === 1 ? "" : "s"}
      </summary>
      <div className="work-items">
        {items.map((t, i) => (
          <span className="badge tool" key={i}>
            {t}
          </span>
        ))}
      </div>
    </details>
  );
}
