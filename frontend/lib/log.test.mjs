// node lib/log.test.mjs — checks the NO_CODE marker never survives into the transcript.
import assert from "node:assert/strict";

const { mergeLog, toolLabel, groupLog } = await import("./log.js");

// marker split across streamed deltas, the case render-time stripping missed
let log = [];
for (const chunk of ["NO", "_CODE", ": The code ", "here is App.tsx"]) {
  log = mergeLog(log, { kind: "token", text: chunk });
}
assert.equal(log.length, 1);
assert.equal(log[0].text, "The code here is App.tsx", `got: ${log[0].text}`);

// marker arriving whole in the first delta
let one = mergeLog([], { kind: "token", text: "NO_CODE: hello" });
assert.equal(one[0].text, "hello");

// done-summaries are cleaned too
assert.equal(mergeLog([], { kind: "ok", text: "NO_CODE: done" })[0].text, "done");

// ordinary text is untouched, including text that merely mentions the marker later
let plain = mergeLog([], { kind: "token", text: "this mentions NO_CODE: inline" });
assert.equal(plain[0].text, "this mentions NO_CODE: inline");

// thinking is left verbatim
assert.equal(mergeLog([], { kind: "think", text: "NO_CODE: musing" })[0].text, "NO_CODE: musing");

// tool labels name the target rather than the raw tool name
assert.deepEqual(toolLabel("read_file", { path: "src/app/App.tsx" }), {
  text: "Read",
  detail: "App.tsx",
});
assert.deepEqual(toolLabel("run_command", { program: "npm", args: ["test", "-s"] }), {
  text: "Ran",
  detail: "npm test -s",
});
assert.equal(toolLabel("list_files", {}).text, "list_files");

// consecutive tool calls fold into one group, other items break the run
const groups = groupLog([
  { kind: "user", text: "go" },
  { kind: "tool", text: "Read", detail: "a.ts" },
  { kind: "tool", text: "Read", detail: "b.ts" },
  { kind: "token", text: "done" },
]);
assert.equal(groups.length, 3);
assert.equal(groups[1].items.length, 2);

console.log("log: all checks passed");

// --- blank assistant turns must not split tool groups or render a copy row ---
{
  const log = [
    { kind: "user", text: "go" },
    { kind: "tool", text: "Ran", detail: "a" },
    { kind: "token", text: "" },          // tool-only turn
    { kind: "tool", text: "Ran", detail: "b" },
    { kind: "token", text: "   \n " },    // whitespace only
    { kind: "tool", text: "Ran", detail: "c" },
    { kind: "token", text: "Done." },
  ];
  const g = groupLog(log);
  assert.equal(g.length, 3, `expected user + one tool group + text, got ${JSON.stringify(g.map(x => x.kind))}`);
  assert.equal(g[1].items.length, 3, "the three tool calls must merge into one group");
  assert.equal(g[2].text, "Done.");
  assert.ok(!g.some(x => x.kind === "token" && !x.text.trim()), "no blank text blocks survive");
}

console.log("log: blank-turn checks passed");
