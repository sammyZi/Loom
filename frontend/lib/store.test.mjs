// node lib/store.test.mjs  — checks session persistence without a browser
import assert from "node:assert/strict";

// minimal localStorage stand-in with an optional quota
let quota = Infinity;
const mem = new Map();
globalThis.localStorage = {
  getItem: (k) => mem.get(k) ?? null,
  setItem: (k, v) => {
    if (v.length > quota) throw new Error("QuotaExceededError");
    mem.set(k, v);
  },
};

const { loadSessions, saveSession, deleteSession, newSessionId, whenText } = await import(
  "./store.js"
);

const A = "D:/proj/a";
const B = "D:/proj/b";
const mk = (id, folder, at, title = "t") => ({ id, folder, title, log: [{ kind: "ok", text: "x" }], at });

// scoped per folder
saveSession(mk("1", A, 100));
saveSession(mk("2", B, 200));
assert.deepEqual(loadSessions(A).map((s) => s.id), ["1"]);
assert.deepEqual(loadSessions(B).map((s) => s.id), ["2"]);

// upsert, not duplicate; newest first
saveSession(mk("1", A, 300, "renamed"));
assert.equal(loadSessions(A).length, 1);
assert.equal(loadSessions(A)[0].title, "renamed");
saveSession(mk("3", A, 400));
assert.deepEqual(loadSessions(A).map((s) => s.id), ["3", "1"]);

// delete only touches the one id
deleteSession("1", A);
assert.deepEqual(loadSessions(A).map((s) => s.id), ["3"]);
assert.equal(loadSessions(B).length, 1);

// capped at 40 newest
for (let i = 0; i < 60; i++) saveSession(mk(`bulk${i}`, A, 1000 + i));
const kept = loadSessions(A);
assert.ok(kept.length <= 40, `expected <=40, got ${kept.length}`);
assert.equal(kept[0].id, "bulk59");

// quota blowout drops the oldest half instead of throwing
quota = mem.get("ide-ai-sessions").length / 2;
assert.doesNotThrow(() => saveSession(mk("tight", A, 99999)));
assert.ok(loadSessions(A).length > 0);

// ids are unique
assert.notEqual(newSessionId(), newSessionId());

// relative time
assert.equal(whenText(Date.now()), "just now");
assert.equal(whenText(Date.now() - 5 * 60000), "5m ago");
assert.equal(whenText(Date.now() - 3 * 3600000), "3h ago");
assert.equal(whenText(Date.now() - 2 * 86400000), "2d ago");

console.log("store: all checks passed");
