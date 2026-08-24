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

// --- voice choice: Listen should speak in a female voice -------------------
// Mirrors humanVoice in components/Markdown.tsx. The Web Speech API has no
// gender field, so the ranking works off the shipped voice names.
const FEMALE =
  /\b(female|aria|jenny|michelle|ana|sonia|libby|natasha|clara|emily|zira|hazel|eva|samantha|victoria|karen|moira|tessa|fiona|allison|ava|susan|zoe|serena|catherine|linda|heather|amber|ashley|cora|elizabeth|monica|nova|joanna|salli|kendra|kimberly|ivy)\b/;
const MALE =
  /\b(male|david|mark|guy|ryan|george|christopher|eric|brian|daniel|alex|fred|tom|oliver|william|liam|steffan|roger|sam|arthur|thomas|gordon|james|jason|nathan|aaron|matthew|joey|justin|kevin)\b/;

function humanVoice(voices) {
  const en = voices.filter((v) => v.lang.toLowerCase().startsWith("en"));
  if (!en.length) return null;
  const gender = (n) => (FEMALE.test(n) ? 0 : MALE.test(n) ? 2 : 1);
  const quality = (v, n) => {
    if (n.includes("natural")) return 0;
    if (n.includes("google")) return 1;
    if (/\b(david|zira|mark|sam)\b/.test(n)) return 4;
    if (v.localService) return 2;
    return 3;
  };
  const score = (v) => {
    const n = v.name.toLowerCase();
    return gender(n) * 10 + quality(v, n);
  };
  return [...en].sort((a, b) => score(a) - score(b))[0] ?? null;
}

const v = (name, lang = "en-US", localService = true) => ({ name, lang, localService });

// A typical Windows 11 list: the male natural voice must not win.
const windows = [
  v("Microsoft David - English (United States)"),
  v("Microsoft Zira - English (United States)"),
  v("Microsoft Guy Online (Natural) - English (United States)", "en-US", false),
  v("Microsoft Aria Online (Natural) - English (United States)", "en-US", false),
];
assert.match(humanVoice(windows).name, /Aria/, "a female natural voice should win");

// Quality still decides between two female voices.
assert.match(
  humanVoice([v("Microsoft Zira - English (United States)"), windows[3]]).name,
  /Aria/,
  "natural beats the flat legacy voice",
);

// Chrome's naming carries the gender in the name itself.
assert.match(
  humanVoice([v("Google UK English Male", "en-GB", false), v("Google UK English Female", "en-GB", false)]).name,
  /Female/,
  "explicitly female name should win",
);

// Nothing female installed: still speaks rather than falling silent.
const maleOnly = humanVoice([v("Microsoft David - English (United States)"), v("Daniel", "en-GB")]);
assert.ok(maleOnly, "must fall back to a male voice rather than return null");

// Non-English voices are not candidates.
assert.equal(humanVoice([v("Microsoft Hedda", "de-DE")]), null, "no English voice, no pick");

console.log("speech: ok");
