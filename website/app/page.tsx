import Image from "next/image";
import { Logo } from "@/components/logo";
import shot from "@/images/ss.png";

import { Ticker, Nav, Footer, WAITLIST, REPO } from "@/components/chrome";

const facts = [
  { value: "1", label: "binary — window, server & agent", tint: "bg-coral" },
  { value: "0", label: "accounts · keys stay local", tint: "bg-gold" },
  { value: "14", label: "model providers in the catalog", tint: "bg-mint" },
  { value: "94%", label: "prompt-cache reuse by turn two", tint: "bg-sky-blue" },
];

const sources = [
  { icon: "▤", label: "Any folder", tint: "bg-coral" },
  { icon: "⎇", label: "Git repo", tint: "bg-gold" },
  { icon: "◈", label: "14 providers", tint: "bg-crimson" },
];

const outputs = [
  { icon: "±", label: "Reviewed diffs", tint: "bg-mint" },
  { icon: "❯", label: "Terminal output", tint: "bg-sky-blue" },
  { icon: "✓", label: "Commits", tint: "bg-periwinkle-mist" },
];

const features = [
  {
    icon: "▤",
    tint: "bg-sky-blue",
    title: "Terminal panel",
    body: "Live streaming output, real stdin, Ctrl+C that works. One tab per background job, killed with the tree.",
    wide: false,
  },
  {
    icon: "±",
    tint: "bg-mint",
    title: "Changes panel",
    body: "Per-file diffs against git. Review the work, stage what you like, commit from the same view.",
    wide: false,
  },
  {
    icon: "◱",
    tint: "bg-gold",
    title: "Browser panel",
    body: "An iframe and a URL bar for checking the dev server the agent just started — without leaving the app.",
    wide: false,
  },
  {
    icon: "⌗",
    tint: "bg-periwinkle-mist",
    title: "Sessions",
    body: "Every chat is stored locally in SQLite. Searchable, archivable, yours. Nothing syncs anywhere.",
    wide: false,
  },
  {
    icon: "◇",
    tint: "bg-crimson",
    title: "Code graph",
    body: "The repo is indexed as a graph, so the agent jumps to the definition instead of grepping the whole tree.",
    wide: false,
  },
  {
    icon: "⛨",
    tint: "bg-coral",
    title: "A sandbox, not vibes",
    body: "Commands run in a Job Object with a restricted token and process caps. Installs and clones still work like a normal shell; the whole tree is killed on timeout or cancel.",
    wide: true,
  },
];

const rules = [
  { pattern: "git status", verdict: "allow", tint: "bg-mint" },
  { pattern: "cargo test", verdict: "allow", tint: "bg-mint" },
  { pattern: "*", verdict: "ask", tint: "bg-gold" },
  { pattern: "git push", verdict: "deny", tint: "bg-coral" },
];

const costStats = [
  {
    display: "94%",
    pct: 94,
    label: "of the prompt is cache-read by turn two",
    tint: "bg-sky-blue",
  },
  {
    display: "~⅒×",
    pct: 10,
    label: "of the billed cost on those cached turns",
    tint: "bg-mint",
  },
  {
    display: "@85%",
    pct: 85,
    label: "context used before history is summarised",
    tint: "bg-gold",
  },
];

const faqs = [
  {
    q: "Does my code leave my machine?",
    a: "Only the parts the model needs, and only to the provider whose key you configured. There is no Loom server, no account, no telemetry and no sync — the binary talks to your filesystem and to one API endpoint you chose.",
  },
  {
    q: "What stops it from running something destructive?",
    a: "Two things. Every command is matched against ordered permission rules, so git push can be denied outright while git status runs unattended. And whatever is allowed runs inside a Job Object with a restricted token and process caps, so a runaway install cannot outlive the cancel button.",
  },
  {
    q: "What happens when it gets something wrong?",
    a: "You see it before it matters. Every tool call is listed as it happens, edits land as per-file diffs against git, and a step can be undone from the Changes panel. Nothing is staged or committed unless you press the button.",
  },
  {
    q: "Who pays for the models, and how much?",
    a: "You do, directly to the provider — Loom adds no margin because it has no server. The system prompt is a cached prefix reused at about 94% by the second turn, so most turns are billed at roughly a tenth of the naive cost.",
  },
  {
    q: "Does it hold up on a large repo?",
    a: "The repo is indexed as a code graph, so the agent jumps to a definition instead of grepping the tree. Files it has already read collapse to one-line pointers until they change, and old tool output is pruned before the history is summarised.",
  },
  {
    q: "How is this different from an editor plugin?",
    a: "The agent is the application, not a sidebar bolted onto one. The terminal, the diffs, the browser preview and the session history are panels around the same loop, so you review the work where it happened instead of guessing what a chat box did.",
  },
  {
    q: "Can I stop it from editing at all?",
    a: "Yes. Agents are presets over the same rules: Build runs unrestricted, Plan reads and reasons without touching a file, and Explore subagents are read-only. A denied tool is dropped from the model's schema, so it never calls it and never argues about it.",
  },
  {
    q: "What does beta mean here?",
    a: "It works, and it is not finished. Windows only for now — everything except the sandbox is portable Rust, so the rest is a port rather than a rewrite. Expect rough edges in the UI and the occasional bad day.",
  },
  {
    q: "Is it open source?",
    a: "Source-available, not open source. Free for individuals under the PolyForm Noncommercial License — read it, fork it, build it yourself. Companies using it for paid work need a commercial license.",
  },
];

function Wash({ className }: { className?: string }) {
  return (
    <div
      aria-hidden
      className={`pointer-events-none absolute -z-10 rounded-full blur-[75px] ${className}`}
    />
  );
}

function Eyebrow({ n, children }: { n: string; children: React.ReactNode }) {
  return (
    <p className="text-caption uppercase text-smoke">
      <span className="text-off-black">{n}</span> — {children}
    </p>
  );
}


/** Hand-drawn brush stroke under the headline claim. */
function Swash() {
  return (
    <svg
      className="swash"
      viewBox="0 0 300 18"
      preserveAspectRatio="none"
      aria-hidden
    >
      <linearGradient id="brush" x1="0" x2="1">
        <stop offset="0" stopColor="#ff9473" />
        <stop offset="0.55" stopColor="#f37a0a" />
        <stop offset="1" stopColor="#ecda98" />
      </linearGradient>
      <path
        fill="url(#brush)"
        d="M1.5 11.6c22-3.1 58-5.4 96-6.3 44-1 92 .3 137 2.4 22 1 44 2.3 64.4 4.2-13.7 1.4-27.6 1.1-41.4.6-38-1.4-76-3.6-114-3.4-31 .2-62 1.6-92.8 4.6-16 1.5-32 3.4-47.7 5.2-1.9.2-3-2.3-1.5-3.1 3.6-2 8-3.2 12-4.2 5.6-1.4-4.7 0-11.6 0z"
      />
      <path
        fill="url(#brush)"
        opacity="0.5"
        d="M232 15.4c18 .3 36 .9 53.6 2.1-16.4.6-33 .2-49.4-.7-2.9-.2-5-1.5-4.2-1.4z"
      />
    </svg>
  );
}

/** A connector: the static line, plus a packet that travels it. */
function Wire({
  d,
  stroke,
  delay,
}: {
  d: string;
  stroke: string;
  delay: number;
}) {
  return (
    <g fill="none" stroke={stroke}>
      <path d={d} opacity="0.3" />
      <path
        d={d}
        pathLength={100}
        className="flow"
        style={{ "--d": `${delay}s` } as React.CSSProperties}
      />
    </g>
  );
}

function Node({
  icon,
  label,
  tint,
}: {
  icon: string;
  label: string;
  tint: string;
}) {
  return (
    <span className="tag">
      <span
        className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-pill text-caption ${tint}`}
      >
        {icon}
      </span>
      {label}
    </span>
  );
}

export default function Home() {
  return (
    <>
      <Ticker />
      <Nav />

      <main>
        {/* hero */}
        <section className="relative isolate overflow-hidden px-5 pt-14 pb-20 text-center sm:px-6 sm:pt-24 sm:pb-28">
          <Wash className="top-[-200px] left-[4%] h-[300px] w-[320px] bg-linear-to-br from-coral/80 via-gold/80 to-crimson/60 opacity-45 sm:left-[8%] sm:h-[440px] sm:w-[560px]" />
          <Wash className="top-[-160px] right-[4%] h-[280px] w-[300px] bg-linear-to-bl from-sky-blue/80 via-periwinkle-mist to-mint/80 opacity-50 sm:right-[6%] sm:h-[400px] sm:w-[520px]" />

          <h1
            className="rise mx-auto max-w-5xl font-serif text-[36px] leading-[1.15] tracking-[-0.02em] sm:text-[60px] lg:text-[72px]"
            style={{ "--d": "0.08s" } as React.CSSProperties}
          >
            There is{" "}
            <span className="mark">
              no server
              <Swash />
            </span>
            .
            <br />
            <span className="text-graphite">Just your folder.</span>
          </h1>
          <p
            className="rise mx-auto mt-6 max-w-2xl text-body text-graphite sm:mt-8 sm:text-body-lg"
            style={{ "--d": "0.16s" } as React.CSSProperties}
          >
            A coding agent with an IDE around it. One binary, your keys, your
            machine.
          </p>
          <div
            className="rise mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row sm:gap-4"
            style={{ "--d": "0.24s" } as React.CSSProperties}
          >
            <a
              href={WAITLIST}
              target="_blank"
              rel="noreferrer"
              className="btn-blue w-full sm:w-auto"
            >
              Join the waitlist <span className="arrow">▸</span>
            </a>
            <a href="#pipeline" className="btn-ghost w-full sm:w-auto">
              See how it works
            </a>
          </div>
          <p
            className="rise mt-5 text-caption uppercase text-smoke"
            style={{ "--d": "0.3s" } as React.CSSProperties}
          >
            one binary · windows · free for individuals
          </p>

          {/* product shot */}
          <div
            className="rise mx-auto mt-14 max-w-6xl rounded-[24px] border border-ash p-1.5 sm:mt-20 sm:rounded-card sm:p-2"
            style={{ "--d": "0.36s" } as React.CSSProperties}
          >
            <Image
              src={shot}
              alt="The Loom desktop app: session list on the left, the agent's tool calls and summary in the centre, a commit bar and prompt box at the bottom."
              priority
              className="h-auto w-full rounded-[18px] sm:rounded-[32px]"
            />
          </div>
        </section>

        {/* facts */}
        <section className="border-y border-ash">
          <div className="mx-auto grid max-w-[1432px] grid-cols-2 gap-x-6 gap-y-10 px-5 py-12 sm:px-6 sm:py-16 md:grid-cols-4">
            {facts.map((f) => (
              <div key={f.label} className="reveal">
                <div className={`mb-4 h-1 w-8 rounded-pill sm:w-10 ${f.tint}`} />
                <div className="font-serif text-heading-sm sm:text-heading">
                  {f.value}
                </div>
                <div className="mt-2 max-w-[200px] text-body-sm text-graphite">
                  {f.label}
                </div>
              </div>
            ))}
          </div>
        </section>

        {/* pipeline diagram */}
        <section
          id="pipeline"
          className="relative isolate mx-auto max-w-[1432px] scroll-mt-20 px-5 py-16 sm:px-6 sm:py-28"
        >
          <div className="reveal">
            <Eyebrow n="01">how it works</Eyebrow>
          <h2 className="mt-5 max-w-2xl font-serif text-heading-sm sm:text-heading-lg">
            One loop, all of it visible
          </h2>
          <p className="mt-4 max-w-xl text-body text-graphite sm:text-body-lg">
            The agent reads files, writes files, runs commands and searches the
            web — then reports what changed. Every tool call is shown; nothing
            happens off-stage.
            </p>
          </div>

          <div className="reveal mt-12 grid items-center gap-8 sm:mt-20 md:grid-cols-[auto_1fr_auto_1fr_auto] md:gap-10">
            <div className="flex flex-col items-center gap-4 md:items-start md:gap-6">
              {sources.map((s) => (
                <Node key={s.label} {...s} />
              ))}
            </div>

            <svg
              viewBox="0 0 120 200"
              preserveAspectRatio="none"
              className="hidden h-[200px] w-full md:block"
              aria-hidden
            >
              <linearGradient id="flow-in" x1="0" x2="1">
                <stop offset="0" stopColor="#ff9473" />
                <stop offset="1" stopColor="#a0b5eb" />
              </linearGradient>
              {[20, 100, 180].map((y, i) => (
                <Wire
                  key={y}
                  d={`M0 ${y} C60 ${y} 60 100 120 100`}
                  stroke="url(#flow-in)"
                  delay={i * 0.55}
                />
              ))}
            </svg>

            {/* mobile connectors — the curves only make sense side by side */}
            <div
              aria-hidden
              className="mx-auto h-10 w-px bg-linear-to-b from-coral to-sky-blue md:hidden"
            />

            <div className="relative mx-auto">
              <Wash className="breathe inset-0 m-auto h-40 w-40 bg-mint opacity-70" />
              <div className="flex h-36 w-36 flex-col items-center justify-center gap-2 rounded-pill border border-ash bg-parchment text-center sm:h-40 sm:w-40">
                <Logo size={26} />
                <span className="text-caption uppercase">Loom agent</span>
                <span className="text-caption uppercase text-smoke">
                  sandboxed
                </span>
              </div>
            </div>

            <div
              aria-hidden
              className="mx-auto h-10 w-px bg-linear-to-b from-sky-blue to-mint md:hidden"
            />

            <svg
              viewBox="0 0 120 200"
              preserveAspectRatio="none"
              className="hidden h-[200px] w-full md:block"
              aria-hidden
            >
              <linearGradient id="flow-out" x1="0" x2="1">
                <stop offset="0" stopColor="#a0b5eb" />
                <stop offset="1" stopColor="#a7fccd" />
              </linearGradient>
              {[20, 100, 180].map((y, i) => (
                <Wire
                  key={y}
                  d={`M0 100 C60 100 60 ${y} 120 ${y}`}
                  stroke="url(#flow-out)"
                  delay={1.3 + i * 0.55}
                />
              ))}
            </svg>

            <div className="flex flex-col items-center gap-4 md:items-end md:gap-6">
              {outputs.map((o) => (
                <Node key={o.label} {...o} />
              ))}
            </div>
          </div>
        </section>

        {/* features */}
        <section
          id="features"
          className="mx-auto max-w-[1432px] scroll-mt-20 px-5 py-16 sm:px-6 sm:py-28"
        >
          <div className="reveal flex flex-wrap items-end justify-between gap-6">
            <div>
              <Eyebrow n="02">what is in the window</Eyebrow>
              <h2 className="mt-5 max-w-2xl font-serif text-heading-sm sm:text-heading-lg">
                An IDE, not a chat box
              </h2>
            </div>
            <p className="max-w-sm text-body text-graphite">
              Everything the agent touches lands in a panel you already know how
              to use — and every panel is one keystroke away.
            </p>
          </div>

          <div className="mt-10 grid gap-4 sm:mt-14 sm:gap-6 md:grid-cols-2 lg:grid-cols-3">
            {/* the one colored card */}
            <article className="reveal relative overflow-hidden rounded-[28px] bg-periwinkle-mist p-8 sm:rounded-card sm:p-10 md:col-span-2">
              <span className="flex h-11 w-11 items-center justify-center rounded-pill bg-parchment text-body-lg">
                ✳
              </span>
              <h3 className="mt-6 max-w-sm font-serif text-subheading">
                Chat that edits your repo
              </h3>
              <p className="mt-3 max-w-md text-body text-graphite">
                Ask in plain English. The agent plans, calls its tools, and comes
                back with a summary you can check line by line — plus the diff
                that proves it.
              </p>
              <div className="mt-8 flex flex-wrap gap-2">
                {["edit_file", "run_command", "web_search", "task → explore"].map(
                  (t) => (
                    <span
                      key={t}
                      className="rounded-pill border border-off-black/15 px-4 py-2 text-caption"
                    >
                      {t}
                    </span>
                  ),
                )}
              </div>
              <div
                aria-hidden
                className="absolute -right-16 -bottom-20 h-72 w-72 rounded-full bg-conic from-coral via-sky-blue to-mint opacity-60 blur-[50px]"
              />
            </article>

            {features.map((f) => (
              <article
                key={f.title}
                className={`card reveal rounded-[28px] p-8 sm:rounded-card sm:p-10 ${
                  f.wide ? "md:col-span-2" : ""
                }`}
              >
                <span
                  className={`flex h-11 w-11 items-center justify-center rounded-pill text-body-lg ${f.tint}`}
                >
                  {f.icon}
                </span>
                <h3 className="mt-6 font-serif text-subheading">{f.title}</h3>
                <p className="mt-3 max-w-md text-body text-graphite">{f.body}</p>
              </article>
            ))}
          </div>
        </section>

        {/* permissions */}
        <section
          id="permissions"
          className="relative isolate scroll-mt-20 overflow-hidden border-y border-ash py-16 sm:py-28"
        >
          <Wash className="top-[-120px] right-[-80px] h-[380px] w-[520px] bg-linear-to-bl from-gold/80 to-coral/70 opacity-40" />
          <div className="mx-auto grid max-w-[1432px] items-center gap-10 px-5 sm:px-6 md:grid-cols-2 md:gap-16">
            <div className="reveal">
              <Eyebrow n="03">permissions</Eyebrow>
              <h2 className="mt-5 font-serif text-heading-sm sm:text-heading">
                Per call, not per tool
              </h2>
              <p className="mt-5 text-body text-graphite sm:text-body-lg">
                Every tool call is matched against ordered rules and the last
                match wins. A denied tool is dropped from the model&rsquo;s
                schema — it never calls it, and never has to explain why it
                can&rsquo;t.
              </p>
              <ul className="mt-8 space-y-3 text-body-sm text-graphite">
                {[
                  ["Build", "runs unrestricted, asks on anything destructive"],
                  ["Plan", "reads and reasons, cannot edit a file"],
                  ["Explore", "read-only subagents, no writes at all"],
                ].map(([name, desc]) => (
                  <li key={name} className="flex gap-3">
                    <span className="mt-2 h-1.5 w-1.5 shrink-0 rounded-pill bg-off-black" />
                    <span>
                      <span className="uppercase text-off-black">{name}</span> —{" "}
                      {desc}
                    </span>
                  </li>
                ))}
              </ul>
            </div>

            <div className="card reveal rounded-[28px] bg-parchment p-6 sm:rounded-card sm:p-10">
              <div className="flex items-center justify-between gap-4 text-caption uppercase text-smoke">
                permissions.json
                <span>last match wins ↓</span>
              </div>
              <ul className="mt-6">
                {rules.map((r) => (
                  <li
                    key={r.pattern}
                    className="flex items-center justify-between gap-4 border-t border-ash py-4 text-body-sm"
                  >
                    <code className="truncate">{r.pattern}</code>
                    <span
                      className={`shrink-0 rounded-pill px-4 py-1.5 text-caption uppercase ${r.tint}`}
                    >
                      {r.verdict}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </section>

        {/* cost */}
        <section
          id="cost"
          className="relative isolate mx-auto max-w-[1432px] scroll-mt-20 overflow-hidden px-5 py-16 sm:px-6 sm:py-28"
        >
          <Wash className="bottom-[-140px] left-[-60px] h-[360px] w-[480px] bg-linear-to-tr from-mint/80 to-sky-blue/70 opacity-45" />
          <div className="grid items-center gap-10 md:grid-cols-2 md:gap-16">
            <div className="reveal">
              <Eyebrow n="04">cost</Eyebrow>
              <h2 className="mt-5 font-serif text-heading-sm sm:text-heading">
                Cheap to run,
                <br />
                boring about money
              </h2>
              <p className="mt-5 text-body text-graphite sm:text-body-lg">
                The system prompt is the cached prefix. Files already read return
                one-line pointers until they change, old tool output is pruned
                before history is summarised, and a round that writes nothing
                ends the loop instead of burning two more.
              </p>
              </div>

            <div className="card reveal rounded-[28px] bg-parchment p-6 sm:rounded-card sm:p-10">
              <div className="text-caption uppercase text-smoke">
                measured on a real session
              </div>
              <div className="mt-8 space-y-7">
                {costStats.map((c) => (
                  <div key={c.display}>
                    <div className="flex items-baseline justify-between gap-4">
                      <span className="font-serif text-heading-sm">
                        {c.display}
                      </span>
                      <span className="max-w-[200px] text-right text-body-sm text-graphite">
                        {c.label}
                      </span>
                    </div>
                    <div className="mt-3 h-2 rounded-pill bg-ash/60">
                      <div
                        className={`h-2 rounded-pill ${c.tint}`}
                        style={{ width: `${c.pct}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* faq */}
        <section className="mx-auto max-w-4xl px-5 py-16 sm:px-6 sm:py-28">
          <div className="reveal">
            <Eyebrow n="05">questions</Eyebrow>
          <h2 className="mt-5 font-serif text-heading-sm sm:text-heading-lg">
            Before you sign up
            </h2>
          </div>
          <div className="mt-10">
            {faqs.map((f) => (
              /* name= makes it an exclusive accordion natively — one open at a time */
              <details key={f.q} name="faq" className="faq reveal border-b border-ash">
                <summary className="flex cursor-pointer list-none items-center gap-6 py-6 transition-colors duration-200 hover:text-graphite sm:py-8">
                  <span className="font-serif text-body-lg sm:text-subheading">
                    {f.q}
                  </span>
                  <span className="chev ml-auto text-body-lg">
                    ↓
                  </span>
                </summary>
                <p className="faq-body max-w-2xl pb-8 text-body text-graphite">
                  {f.a}
                </p>
              </details>
            ))}
          </div>
        </section>

        {/* waitlist */}
        <section
          id="waitlist"
          className="relative isolate scroll-mt-20 overflow-hidden border-t border-ash px-5 py-20 text-center sm:px-6 sm:py-32"
        >
          <Wash className="bottom-[-220px] left-1/2 h-[320px] w-[380px] -translate-x-1/2 bg-linear-to-r from-sky-blue/80 via-periwinkle-mist to-mint/80 opacity-50 sm:h-[420px] sm:w-[720px]" />
          <h2 className="reveal mx-auto max-w-2xl font-serif text-heading-sm sm:text-heading-lg">
            Open a folder. Give it a task.
          </h2>
          <p className="mx-auto mt-4 max-w-md text-body text-graphite sm:text-body-lg">
            Windows only for now, and invites go out in batches. Leave your email
            on the form and you get the next one.
          </p>

          <div className="mt-9 flex flex-col items-center justify-center gap-3 sm:flex-row sm:gap-4">
            <a
              href={WAITLIST}
              target="_blank"
              rel="noreferrer"
              className="btn-blue w-full sm:w-auto"
            >
              Join the waitlist <span className="arrow">▸</span>
            </a>
            <a
              href={REPO}
              className="btn-ghost w-full sm:w-auto"
            >
              Read the source
            </a>
          </div>

          <p className="mt-8 text-caption uppercase text-smoke">
            no public download yet · free for individuals · source on GitHub
          </p>
        </section>
      </main>

      <Footer />
    </>
  );
}
