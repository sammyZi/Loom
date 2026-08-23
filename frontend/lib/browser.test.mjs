// node lib/browser.test.mjs — what people type in the address bar must work.
import assert from "node:assert/strict";

// same logic as normalizeUrl in components/BrowserPanel.tsx
function normalizeUrl(input) {
  const s = input.trim();
  if (!s) return "";
  if (/^https?:\/\//i.test(s)) return s;
  if (s.startsWith("/")) return `http://localhost:3000${s}`;
  return `http://${s}`;
}

// the case this exists for: a bare host:port from a dev server's own output
assert.equal(normalizeUrl("localhost:3000"), "http://localhost:3000");
assert.equal(normalizeUrl("127.0.0.1:5173"), "http://127.0.0.1:5173");
assert.equal(normalizeUrl("localhost:3001/about"), "http://localhost:3001/about");

// already absolute: left alone, either scheme, any casing
assert.equal(normalizeUrl("http://x.dev"), "http://x.dev");
assert.equal(normalizeUrl("https://x.dev/a?b=1"), "https://x.dev/a?b=1");
assert.equal(normalizeUrl("HTTPS://X.dev"), "HTTPS://X.dev");

// a bare path is relative to the usual dev server rather than becoming a host
assert.equal(normalizeUrl("/about"), "http://localhost:3000/about");

// whitespace is forgiven; empty stays empty so the panel shows its empty state
assert.equal(normalizeUrl("  localhost:3000  "), "http://localhost:3000");
assert.equal(normalizeUrl(""), "");
assert.equal(normalizeUrl("   "), "");

// http, not https, for bare hosts: these are local servers far more often than
// public sites, and guessing https breaks every dev server.
assert.ok(normalizeUrl("example.com").startsWith("http://"));

console.log("browser: all checks passed");
