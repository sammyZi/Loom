// node lib/speech.test.mjs — markdown must not be spelled out by the voice.
import assert from "node:assert/strict";

// same logic as speakable in components/Markdown.tsx
function speakable(md) {
  const src = md.trim();
  if (!src) return "";
  return src
    .replace(/```[\s\S]*?```/g, " code block. ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/(\*\*|__|\*|_|~~)/g, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/\|/g, " ")
    .replace(/\n{2,}/g, ". ")
    .replace(/\s+/g, " ")
    .replace(/^[.\s]+/, "")
    .trim();
}

// the point of the whole thing: a fenced block becomes four words, not a recital
const withCode = speakable("Here:\n\n```js\nconst x = 1;\nfor (;;) {}\n```\n\nDone.");
assert.ok(!withCode.includes("const"), `code leaked: ${withCode}`);
assert.ok(withCode.includes("code block"), withCode);
assert.ok(withCode.startsWith("Here:"), withCode);
assert.ok(withCode.endsWith("Done."), withCode);

// headings, emphasis and bullets lose their punctuation but keep their words
assert.equal(speakable("## Big news"), "Big news");
assert.equal(speakable("**bold** and _thin_ and ~~gone~~"), "bold and thin and gone");
assert.equal(speakable("- one\n- two"), "one two");
assert.equal(speakable("> quoted"), "quoted");

// links read as their text, images say nothing at all
assert.equal(speakable("see [the docs](https://example.com/x)"), "see the docs");
assert.equal(speakable("![a picture](/img.png)"), "");

// inline code keeps its content, since it is usually a real word
assert.equal(speakable("run `npm test` now"), "run npm test now");

// tables collapse instead of reading pipes aloud
assert.ok(!speakable("| a | b |\n| - | - |").includes("|"));

// a hash inside a sentence is not a heading
assert.equal(speakable("issue #42 is open"), "issue #42 is open");

// blank input stays blank rather than becoming stray punctuation
assert.equal(speakable(""), "");
assert.equal(speakable("\n\n   \n"), "");

console.log("speech: all checks passed");
