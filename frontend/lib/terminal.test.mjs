// node lib/terminal.test.mjs — terminal tab naming must never collide.
import assert from "node:assert/strict";

// same logic as nextName in components/TerminalPanel.tsx
function nextName(terms) {
  const used = new Set(terms.map((t) => t.name));
  for (let i = 1; ; i++) {
    const name = `Terminal ${i}`;
    if (!used.has(name)) return name;
  }
}

const T = (n) => ({ name: `Terminal ${n}` });

// fresh panel
assert.equal(nextName([]), "Terminal 1");

// sequential adds
assert.equal(nextName([T(1)]), "Terminal 2");
assert.equal(nextName([T(1), T(2)]), "Terminal 3");

// the reported bug: 3 tabs, close 1 and 2, then add.
// Naming off the count gave "Terminal 2"... and later collided with the survivor.
const survivors = [T(3)];
assert.equal(nextName(survivors), "Terminal 1", "should reuse the lowest free slot");

// never duplicates an existing name, whatever the holes are
const holey = [T(1), T(3), T(4)];
const picked = nextName(holey);
assert.equal(picked, "Terminal 2");
assert.ok(!holey.some((t) => t.name === picked));

// closing the middle one frees exactly that slot
assert.equal(nextName([T(1), T(3)]), "Terminal 2");

// same logic as splitLog in components/TerminalPanel.tsx
function splitLog(log, cwd) {
  const echo = `${cwd}> `;
  const lines = log.split("\n");
  const runs = [];
  let buf = "";
  const flush = () => {
    if (buf) {
      runs.push({ kind: "out", text: buf });
      buf = "";
    }
  };
  for (let i = 0; i < lines.length; i++) {
    const nl = i < lines.length - 1 ? "\n" : "";
    const line = lines[i];
    if (line.startsWith(echo)) {
      flush();
      runs.push({ kind: "prompt", text: line + nl });
      continue;
    }
    // A note is only ever its own single line.
    if (/^\[(exited code \d+|stopped|failed[^\]]*)\]\s*$/.test(line)) {
      flush();
      runs.push({ kind: "note", text: line + nl });
      continue;
    }
    buf += line + nl;
  }
  flush();
  return runs;
}

const CWD = "D:\\projects\\keyboard";
// whatever the split, re-joining every run must reproduce the log byte for byte
const rejoin = (log) =>
  splitLog(log, CWD)
    .map((r) => r.text)
    .join("");

// the reported bug: the echoed prompt was one flat run, so the path inherited
// the near-white output colour instead of the prompt colour.
const log = `${CWD}> ls\nAGENTS.md\nCLAUDE.md\n${CWD}> pwd\n${CWD}\n`;
const runs = splitLog(log, CWD);
assert.equal(rejoin(log), log, "must round-trip exactly");
assert.deepEqual(
  runs.map((r) => r.kind),
  ["prompt", "out", "prompt", "out"],
  "only the echoed command lines are prompts",
);
assert.equal(runs[0].text, `${CWD}> ls\n`);

// a bare cwd line in output is NOT a prompt: the echo always has "> " after it
assert.equal(splitLog(`${CWD}\n`, CWD)[0].kind, "out");

// output lines coalesce instead of becoming one run per line
assert.equal(splitLog("a\nb\nc\n", CWD).length, 1);

// empty log, and a log that is nothing but a prompt
assert.deepEqual(splitLog("", CWD), []);
assert.equal(rejoin(`${CWD}> ls`), `${CWD}> ls`, "round-trips without trailing newline");

// exit/stop/failure notes become their own chip runs, never merged into output
const noted = `${CWD}> npm test\nok\n[exited code 0]\n${CWD}> ping -t\n^C\n[stopped]\n`;
const noteRuns = splitLog(noted, CWD);
assert.equal(rejoin(noted), noted, "notes round-trip too");
assert.deepEqual(
  noteRuns.map((r) => r.kind),
  ["prompt", "out", "note", "prompt", "out", "note"],
);

// Agent tab streaming: the backend echoes and streams under id "agent" and the
// UI just concatenates chunks — no more tool_call/tool_result folding, which
// mispaired whenever a command failed between the two events.
function appendChunk(prev, ev) {
  if (ev.type === "chunk" && ev.id === "agent") return prev + ev.text;
  return prev;
}
let a = appendChunk("", { type: "chunk", id: "agent", text: `${CWD}> git status\n` });
assert.equal(a, `${CWD}> git status\n`);
a = appendChunk(a, { type: "chunk", id: "agent", text: "clean\n" });
assert.equal(a, `${CWD}> git status\nclean\n`);
// other terminal ids and non-chunk frames are ignored here
assert.equal(appendChunk("keep", { type: "chunk", id: "t-1", text: "x" }), "keep");
assert.equal(appendChunk("keep", { type: "token", text: "x" }), "keep");
// the streamed echo must be recognised as a prompt by splitLog — these two
// share the `cwd> ` format, so a change to one has to move the other
assert.equal(splitLog(a, CWD)[0].kind, "prompt");

console.log("terminal: all checks passed");
