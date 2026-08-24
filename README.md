# Loom

A local AI coding agent with an IDE around it. One Rust binary is the desktop
app: it serves the API, hosts the embedded web UI in its own window, and runs the
agent itself — open a folder and it works in that folder, on your machine, with
your own provider keys.

```bash
cargo build -p cli
./target/debug/ide-ai.exe        # opens the Loom window (serves http://127.0.0.1:8080)
```

The window is a `tao` window with a `wry` webview pointed at the local server —
no browser, no tabs. On Windows that webview is Edge WebView2, which ships with
Windows 11.

Windows only for now: the sandbox is built on Job Objects and restricted tokens
(see [Sandbox](#sandbox)). Everything else is portable.

---

## What it does

- **Chat with an agent that edits your repo** — reads, writes, runs commands,
  searches the web, and reports what it changed.
- **Terminal panel** with live streaming output, real stdin, Ctrl+C, and a
  separate tab per background job.
- **Changes panel** — per-file diffs against git, with staging and commit.
- **Browser panel** — an iframe and a URL bar for checking the dev server the
  agent just started.
- **Sessions** — every chat is stored in SQLite, searchable, archivable.

---

## Architecture

```
cli/           axum HTTP + websockets, SQLite session store, embedded UI
  main.rs        the desktop window (tao + wry); the server runs behind it
  routes.rs      every endpoint; /ws/agent, /ws/shell, /ws/files
  icon.py        redraws the app's brand mark into icon.ico (exe + window icon)
orchestrator/  run modes: which agents run, in what order, with which permissions
agent/         the model loop, tools, providers, skills, compaction
  loop_.rs       the agentic loop: stream → tool calls → repeat
  tools.rs       every tool the model can call
  provider.rs    provider catalog + the two streaming engines
  skills.rs      SKILL.md discovery and preloading
  agents.rs      agent definitions (Build, Plan, Explore, Scout, General)
  compact.rs     prune old tool output, then summarise, at 85% of the window
sandbox/       process isolation: Job Object, restricted token, no network env
core/          shared types: events, permissions, workspace root, shell registry
viewer/        filesystem, git, and file-watching helpers
frontend/      Next.js UI, statically exported and embedded into the binary
```

### The build chain that catches everyone

`cli` embeds `frontend/out` with `rust_embed` **at compile time**. A frontend
change is invisible to the running app until you rebuild both:

```bash
cd frontend && npx next build     # regenerates frontend/out
cd .. && cargo build -p cli       # re-embeds it
```

For UI work, run the dev server instead — it proxies the API on :8080. Keep the
app running for the backend and open the dev server in a browser; :3000 is
allow-listed for CORS (add others with `IDE_AI_EXTRA_ORIGIN`):

```bash
cd frontend && npm run dev        # http://localhost:3000
```

---

## The agent model

Borrowed from [opencode](https://github.com/anomalyco/opencode). An agent is a
prompt plus a **permission set**, not a hardcoded tool list.

### Permissions

Every tool call is matched against ordered rules; the last match wins.

| value   | behaviour                        |
| ------- | -------------------------------- |
| `allow` | runs without asking              |
| `ask`   | prompts for approval             |
| `deny`  | tool is not offered to the model  |

Patterns match the call's own subject — the command line for shell, the path for
file tools — so control is per-call, not per-tool:

```json
{ "bash": { "*": "ask", "git status": "allow", "git push": "deny" } }
```

A denied tool is dropped from the schema entirely, so the model never calls it
and never has to explain that it cannot.

### Agents

**Primary** (you talk to these): `build` (unrestricted), `plan` (no edits, asks
before running).
**Subagents** (delegated to via the `task` tool): `explore` (read-only codebase),
`scout` (read-only, external docs), `general` (full tools).

A subagent runs in its own context and returns only its final message. It gets no
runner of its own, so delegation cannot recurse.

The composer's Auto / Plan / Manual / Approve modes are presets over this.

### Tools

`read_file` (with offset/limit), `write_file`, `edit_file`, `list_files`,
`search_files`, `run_command` (foreground or background), `check_code`,
`run_tests`, `web_search`, `web_fetch`, `browser_open`, `todo_write`,
`ask_user`, `task`, `skill`.

---

## Code graph

[graphify](https://github.com/Graphify-Labs/graphify) turns the repo into a
knowledge graph the agent can query instead of reading files to find out how
things connect. Extraction is tree-sitter, not an LLM, and every edge is tagged
`EXTRACTED` or `INFERRED`.

```bash
pip install graphifyy
graphify extract . --code-only --cargo   # builds graphify-out/ (no API key)
graphify update .                        # incremental refresh
```

`.opencode/skills/graphify/SKILL.md` teaches the agent when to reach for it —
orientation, "who calls this", blast radius before a refactor — and which
commands to run (`query`, `path`, `explain`, `affected`, `god-nodes`).
`graphify-out/` is gitignored; rebuild it per clone.

---

## Skills

A skill is reusable instructions in a `SKILL.md` with YAML frontmatter. Loom
searches, project before global:

```
.opencode/skills/<name>/SKILL.md
.claude/skills/<name>/SKILL.md
.agents/skills/<name>/SKILL.md
~/.config/opencode/skills/<name>/SKILL.md   (+ ~/.claude, ~/.agents)
```

```markdown
---
name: code-review
description: The house review checklist
---
Steps here.
```

The name must match its folder and be `^[a-z0-9]+(-[a-z0-9]+)*$`. Skills are
**preloaded into the system prompt** up to 16 KB, so they apply without the model
having to remember to ask; the rest stay behind the `skill` tool. Preloading is
cheap because the system prompt is the cached prefix — see below.

---

## Providers

14 providers in the catalog, plus whatever their `/models` endpoint reports.
Configure keys in **Settings** (the gear beside *Clear all sessions*); they are
stored in your user profile, and env vars still work as fallbacks.

Model lists are fetched live where the provider supports it — OpenRouter reports
400+ — falling back to a curated list when it does not. OpenAI-compatible
providers and Anthropic are both supported; an OpenAI-compatible base URL can be
pointed anywhere.

---

## Token behaviour

The expensive part of an agent is re-sending context, not generating text. What
Loom does about it:

- **Prompt caching.** The system prompt and tool schemas are cached — explicitly
  on Anthropic via `cache_control`, automatically on providers that match on a
  stable prefix. Measured at **94% prefix reuse** on turn two, billed at roughly
  a tenth.
- **Read cache.** A file already read this task returns a one-line pointer
  instead of the file. Content-hashed, so an edit makes it fresh again.
- **Read budget.** 60 KB of whole-file content per task; past that, reads must
  use `search_files` or an offset/limit range.
- **Compaction.** Old tool outputs are pruned first (protecting the most recent
  40,000 tokens), then history is summarised, triggering at 85% of the window.
- **Bounded rounds.** A coder round that writes nothing ends the loop instead of
  running two more, and the reviewer gets the changed-file list rather than the
  whole project.

Set `RUST_LOG=info` to see cache hit rates and per-read cache decisions.

---

## Sandbox

Commands run in a Job Object with a restricted token and a process and memory
cap. Network access is real — installs, `git clone`, and package managers work
like a normal shell. The whole process tree is killed on timeout, on cancel,
and when its terminal tab closes; a detached process that outlives its own
command on purpose (an editor opened with `code .` or `kiro .`) is the one
thing spared.

Scratch space lives in the system temp directory, keyed by workspace — **not** in
your project, because a stray folder there breaks tools that require an empty
directory (`create-next-app` refuses to scaffold into one).

---

## Development

```bash
cargo test --workspace            # Rust
cd frontend && npx tsc --noEmit   # types
node lib/browser.test.mjs && node lib/feed.test.mjs \
  && node lib/speech.test.mjs && node lib/terminal.test.mjs
```

Frontend tests are plain `node` scripts with `assert` — no framework. The working
ones mirror the source logic rather than importing it, because the source is
TypeScript and there is no compile step for `lib/`.

Two files do not run and are excluded above: `lib/log.test.mjs` and
`lib/store.test.mjs` both `await import()` a compiled `./log.js` / `./store.js`
that nothing generates. `store.test.mjs` is stale as well as unbuilt — it imports
`loadSessions`, which was renamed to `loadAllSessions`. They need converting to
the mirror convention (or a build step) before they guard anything.
