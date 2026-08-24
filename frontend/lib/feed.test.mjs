// node lib/feed.test.mjs — reasoning must appear exactly once on screen.
import assert from "node:assert/strict";

// same logic as groupLog in lib/log.ts
function groupLog(log) {
  const out = [];
  let think = null;
  for (const l of log) {
    if (l.kind !== "tool" && !l.text.trim()) continue;
    if (l.kind === "user") {
      think = null;
      out.push(l);
      continue;
    }
    if (l.kind === "think") {
      if (think) {
        think.text += `\n${l.text}`;
        continue;
      }
      think = { ...l };
      out.push(think);
      continue;
    }
    if (l.kind !== "tool") {
      out.push(l);
      continue;
    }
    const last = out[out.length - 1];
    if (last && "items" in last) last.items.push(l);
    else out.push({ kind: "tools", items: [l] });
  }
  return out;
}

// same logic as the Feed: one think group per turn, owned by the status line
// while live and by the flow once it settles.
function render(log, busy) {
  const groups = groupLog(log);
  const lastUser = groups.map((g) => ("items" in g ? "" : g.kind)).lastIndexOf("user");
  const thinkAt = groups.findIndex(
    (g, i) => i > lastUser && !("items" in g) && g.kind === "think",
  );
  const liveThink = busy && thinkAt >= 0 ? groups[thinkAt].text : "";
  const flowThinks = groups.filter(
    (g, i) => !(busy && i === thinkAt) && !("items" in g) && g.kind === "think",
  );
  return { groups, liveThink, flowThinks };
}

const think = (text) => ({ kind: "think", text });
const token = (text) => ({ kind: "token", text });
const user = (text) => ({ kind: "user", text });
const tool = () => ({ kind: "tool", text: "Ran" });

const countThinkRows = (groups) =>
  groups.filter((g) => !("items" in g) && g.kind === "think").length;

/// The reported bug: the model reasons, calls tools, reasons again — and each
/// resumption drew its own "Thinking" row, which reads as the same thing twice.
{
  const log = [think("look at the project"), tool(), tool(), think("it is Next.js"), tool()];
  const { groups } = render(log, true);
  assert.equal(countThinkRows(groups), 1, "a turn gets one reasoning block");
  const block = groups.find((g) => !("items" in g) && g.kind === "think");
  assert.ok(block.text.includes("look at the project"), block.text);
  assert.ok(block.text.includes("it is Next.js"), "later reasoning folds in: " + block.text);
}

// merging must not mutate the caller's log
{
  const first = think("a");
  const log = [first, tool(), think("b")];
  render(log, true);
  assert.equal(first.text, "a", "groupLog must copy before appending");
}

// live: the status line owns it, the flow does not repeat it
{
  const { liveThink, flowThinks } = render([token("hi"), think("weighing options")], true);
  assert.equal(liveThink, "weighing options");
  assert.equal(flowThinks.length, 0);
}

// settled: the flow keeps it as the record
{
  const { liveThink, flowThinks } = render([token("hi"), think("weighing options")], false);
  assert.equal(liveThink, "");
  assert.equal(flowThinks.length, 1);
}

// reasoning before a tool call is still owned by exactly one of the two
{
  const { liveThink, flowThinks } = render([think("plan it"), tool(), tool()], true);
  assert.equal(liveThink, "plan it");
  assert.equal(flowThinks.length, 0, "the status line has it, so the flow must not");
}

// a new user message starts a fresh turn with its own block
{
  const log = [think("first turn"), token("answer"), user("again"), think("second turn")];
  const { groups } = render(log, false);
  assert.equal(countThinkRows(groups), 2, "turns do not share a reasoning block");
  const texts = groups.filter((g) => !("items" in g) && g.kind === "think").map((g) => g.text);
  assert.deepEqual(texts, ["first turn", "second turn"]);
}

// no reasoning at all: nothing shown either way
{
  const { liveThink, flowThinks } = render([token("answer")], true);
  assert.equal(liveThink, "");
  assert.equal(flowThinks.length, 0);
}

// consecutive tool calls still collapse into one group
{
  const { groups } = render([tool(), tool(), tool()], false);
  assert.equal(groups.length, 1);
  assert.equal(groups[0].items.length, 3);
}

console.log("feed: all checks passed");

/// The reported bug: two "Thinking" rows on screen at once. Matching the first
/// think group in the session hid turn one's reasoning and left turn two's in
/// the flow next to the live status line, so the same thing appeared twice.
{
  const log = [user("one"), think("first turn"), token("done"), user("two"), think("second turn")];
  const { liveThink, flowThinks } = render(log, true);
  assert.equal(liveThink, "second turn", "the status line shows this turn's reasoning");
  assert.equal(flowThinks.length, 1, "only the settled turn keeps a row");
  assert.equal(flowThinks[0].text, "first turn");
}

// a new turn that has not reasoned yet leaves every earlier block in the flow
{
  const log = [user("one"), think("first turn"), token("done"), user("two")];
  const { liveThink, flowThinks } = render(log, true);
  assert.equal(liveThink, "");
  assert.equal(flowThinks.length, 1);
}
