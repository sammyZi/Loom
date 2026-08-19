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

console.log("terminal: all checks passed");
