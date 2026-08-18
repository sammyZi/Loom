"use client";

import { IconRefresh } from "@/components/Icons";

export function DiffPanel({ diff, onRefresh }: { diff: string; onRefresh: () => void }) {
  const lines = diff ? diff.split("\n") : [];
  return (
    <div className="term">
      <div className="term-tabs">
        <span className="panel-title">Changes</span>
        <span className="spacer" />
        <button className="icon-btn" onClick={onRefresh} title="Refresh">
          <IconRefresh />
        </button>
      </div>
      {lines.length === 0 ? (
        <div className="panel-empty">Working tree clean</div>
      ) : (
        <pre className="diff">
          {lines.map((l, i) => (
            <div key={i} className={lineClass(l)}>
              {l || " "}
            </div>
          ))}
        </pre>
      )}
    </div>
  );
}

function lineClass(l: string) {
  if (l.startsWith("+") && !l.startsWith("+++")) return "add";
  if (l.startsWith("-") && !l.startsWith("---")) return "del";
  if (l.startsWith("@@")) return "hunk";
  if (l.startsWith("diff ") || l.startsWith("index ")) return "meta";
  return "";
}
