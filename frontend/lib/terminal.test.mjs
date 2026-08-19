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
  for (let i = 0; i < lines.length; i++) {
    const nl = i < lines.length - 1 ? "\n" : "";
    if (lines[i].startsWith(echo)) {
      if (buf) {
        runs.push({ prompt: false, text: buf });
        buf = "";
      }
      runs.push({ prompt: true, text: lines[i] + nl });
    } else {
      buf += lines[i] + nl;
    }
  }
  if (buf) runs.push({ prompt: false, text: buf });
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
  runs.map((r) => r.prompt),
  [true, false, true, false],
  "only the echoed command lines are prompts",
);
assert.equal(runs[0].text, `${CWD}> ls\n`);

// a bare cwd line in output is NOT a prompt: the echo always has "> " after it
assert.equal(splitLog(`${CWD}\n`, CWD)[0].prompt, false);

// output lines coalesce instead of becoming one run per line
assert.equal(splitLog("a\nb\nc\n", CWD).length, 1);

// empty log, and a log that is nothing but a prompt
assert.deepEqual(splitLog("", CWD), []);
assert.equal(rejoin(`${CWD}> ls`), `${CWD}> ls`, "round-trips without trailing newline");

// same logic as appendAgentLog in components/TerminalPanel.tsx
// (toolLabel("run_command", …) yields "<program> <args>")
function appendAgentLog(prev, ev, cwd) {
  if (ev.type === "tool_call" && ev.name === "run_command") {
    const detail = [ev.input.program, (ev.input.args || []).join(" ")].filter(Boolean).join(" ");
    return `${prev}${cwd}> ${detail}\n`;
  }
  if (ev.type === "tool_result" && ev.name === "run_command") {
    if (!ev.output) return prev;
    return prev + (ev.output.endsWith("\n") ? ev.output : `${ev.output}\n`);
  }
  return prev;
}

const call = { type: "tool_call", name: "run_command", input: { program: "git", args: ["status"] } };

// a command and its output land in the Agent log, echoed like the user's own
let a = appendAgentLog("", call, CWD);
assert.equal(a, `${CWD}> git status\n`);
a = appendAgentLog(a, { type: "tool_result", name: "run_command", output: "clean" }, CWD);
assert.equal(a, `${CWD}> git status\nclean\n`, "output gets a trailing newline");

// already-terminated output must not gain a second blank line
assert.equal(
  appendAgentLog("", { type: "tool_result", name: "run_command", output: "x\n" }, CWD),
  "x\n",
);
// empty output adds nothing at all
assert.equal(appendAgentLog("keep", { type: "tool_result", name: "run_command", output: "" }, CWD), "keep");

// non-shell work is ignored: the Agent tab is a terminal, not a second feed
for (const ev of [
  { type: "tool_call", name: "read_file", input: { path: "a.ts" } },
  { type: "tool_result", name: "edit_file", output: "wrote" },
  { type: "token", text: "hello" },
  { type: "done", summary: "finished" },
]) {
  assert.equal(appendAgentLog("keep", ev, CWD), "keep", `${ev.type}/${ev.name} must be ignored`);
}

// the echo it writes must be recognised as a prompt by splitLog — these two
// share the `cwd> ` format, so a change to one has to move the other
assert.equal(splitLog(appendAgentLog("", call, CWD), CWD)[0].prompt, true);

console.log("terminal: all checks passed");
