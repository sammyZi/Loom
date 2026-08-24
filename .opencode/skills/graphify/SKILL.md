---
name: graphify
description: Query the repo's code knowledge graph (graphify-out/graph.json) to find how symbols connect, what a change breaks, and where the architectural hubs are. Use before touching unfamiliar code, planning a refactor, or answering "what talks to what" — it is far cheaper than reading files to find out.
---

# graphify

A knowledge graph of this repo built by [graphify](https://github.com/Graphify-Labs/graphify)
with tree-sitter — deterministic, no LLM in the extraction. Every edge is tagged
`EXTRACTED` (found in source) or `INFERRED` (guessed), so weigh them accordingly.

Outputs live in `graphify-out/`:

- `GRAPH_REPORT.md` — hubs, surprising connections, suggested questions. Read this first.
- `graph.json` — the graph the commands below query.

## Commands

Run through `run_command`. All read `graphify-out/graph.json` by default.

```bash
graphify query "what connects the agent loop to the sandbox?" --budget 1500
graphify path "AppState" "Sandbox"     # shortest path between two symbols
graphify explain "WorkspaceRoot"       # a node and its neighbours, in prose
graphify affected "Permission"         # what breaks if this changes (reverse traversal)
graphify god-nodes --top 10            # the most connected symbols
```

## Keeping it current

The graph is a snapshot. Refresh it after the repo has moved:

```bash
graphify update .                             # incremental, code only, no API key
graphify extract . --code-only --cargo        # full rebuild + crate dependency edges
```

If `graphify-out/` does not exist, run the full rebuild once. If `graphify` is not
on PATH, install it with `pip install graphifyy`.

## When to use it

Use it for orientation and blast radius: an unfamiliar area, a refactor, "who
calls this", "what does this module depend on". Skip it when you already know
the file you need — reading that file is cheaper than querying the graph.

Answers are structure, not source. Confirm anything you are about to edit by
reading the actual file.
