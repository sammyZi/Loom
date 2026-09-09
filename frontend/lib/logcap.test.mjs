// node lib/logcap.test.mjs — a terminal's scrollback must stay bounded.
import assert from "node:assert/strict";

const { appendLog, LOG_CAP } = await import("./log.ts");

// under the cap: plain concatenation, byte for byte
assert.equal(appendLog("abc", "def"), "abcdef");
assert.equal(appendLog("", ""), "");

// over the cap: bounded, keeps the newest output, says it trimmed
const big = "x".repeat(LOG_CAP);
const out = appendLog(big, "\nNEWEST-LINE");
assert.ok(out.length <= LOG_CAP + 40, `bounded, got ${out.length}`);
assert.ok(out.endsWith("NEWEST-LINE"), "newest output survives");
assert.ok(out.startsWith("[… earlier output trimmed …]"), "says it trimmed");

// repeated appends stay bounded rather than creeping up
let log = "";
for (let i = 0; i < 200; i++) log = appendLog(log, "line ".repeat(1000) + "\n");
assert.ok(log.length <= LOG_CAP + 40, `stays bounded, got ${log.length}`);
assert.ok(log.endsWith("\n"), "still ends on a line");

console.log("logcap: all checks passed");
