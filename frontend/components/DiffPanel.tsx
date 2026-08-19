"use client";

import { IconChevron, IconRefresh } from "@/components/Icons";
import { baseName } from "@/lib/log";

type FileDiff = { path: string; lines: string[]; add: number; del: number };

/** Split a unified diff into per-file blocks. */
export function splitDiff(diff: string): FileDiff[] {
  const files: FileDiff[] = [];
  let current: FileDiff | null = null;

  for (const line of diff.split("\n")) {
    if (line.startsWith("diff --git")) {
      // "diff --git a/x b/x" -> take the b-side path
      const path = line.split(" b/").pop() || line.replace("diff --git ", "");
      current = { path, lines: [], add: 0, del: 0 };
      files.push(current);
      continue;
    }
    if (!current) continue;
    // drop the noisy preamble; keep hunks and content
    if (
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("new file mode") ||
      line.startsWith("deleted file mode") ||
      line.startsWith("similarity index") ||
      line.startsWith("rename ")
    ) {
      continue;
    }
    if (line.startsWith("+")) current.add++;
    else if (line.startsWith("-")) current.del++;
    current.lines.push(line);
  }
  return files;
}

export function DiffPanel({ diff, onRefresh }: { diff: string; onRefresh: () => void }) {
  const files = splitDiff(diff);

  return (
    <div className="term">
      <div className="term-tabs">
        <span className="panel-title">Changes</span>
        {files.length > 0 && <span className="panel-count">{files.length}</span>}
        <span className="spacer" />
        <button className="icon-btn" onClick={onRefresh} title="Refresh">
          <IconRefresh />
        </button>
      </div>

      {files.length === 0 ? (
        <div className="panel-empty">Working tree clean</div>
      ) : (
        <div className="diff-scroll">
          {files.map((f) => (
            <details className="dfile" key={f.path} open>
              <summary className="dfile-head">
                <IconChevron className="dfile-chev" />
                <span className="dfile-name">{baseName(f.path)}</span>
                <span className="dfile-dir">{dirOf(f.path)}</span>
                <span className="spacer" />
                <span className="dfile-stat">
                  {f.add > 0 && <b className="stat-add">+{f.add}</b>}
                  {f.del > 0 && <b className="stat-del">-{f.del}</b>}
                </span>
              </summary>
              <div className="dfile-body">
                {f.lines.map((l, i) => (
                  <div key={i} className={`dline ${lineClass(l)}`}>
                    <span className="dline-text">{l || " "}</span>
                  </div>
                ))}
              </div>
            </details>
          ))}
        </div>
      )}
    </div>
  );
}

function dirOf(path: string) {
  const parts = path.split("/");
  parts.pop();
  return parts.join("/");
}

function lineClass(l: string) {
  if (l.startsWith("+")) return "add";
  if (l.startsWith("-")) return "del";
  if (l.startsWith("@@")) return "hunk";
  return "";
}
