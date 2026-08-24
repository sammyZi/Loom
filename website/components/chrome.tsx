import { Logo } from "./logo";

export const WAITLIST = "https://tally.so/r/obLbWO";
export const REPO = "https://github.com/sammyZi/loom/";

export const navLinks = [
  { href: "/#pipeline", label: "How it works" },
  { href: "/#features", label: "Features" },
  { href: "/#permissions", label: "Permissions" },
  { href: "/#cost", label: "Cost" },
];

const ticker = [
  "one Rust binary",
  "local-first",
  "your keys, your machine",
  "no accounts, no telemetry",
  "sandboxed commands",
  "sessions in SQLite",
  "14 model providers",
  "written in Rust",
];

const connect = [
  { href: "https://github.com/sammyZi", label: "GitHub" },
  { href: "https://www.linkedin.com/in/samarth-bhinge/", label: "LinkedIn" },
  { href: "https://www.instagram.com/sammyi_57/", label: "Instagram" },
  { href: "mailto:bhingesamarth@gmail.com", label: "Email me" },
];

const project = [
  { href: REPO, label: "Source" },
  { href: "/privacy", label: "Privacy" },
  { href: "/terms", label: "Terms" },
];

/** Announcement ticker — CSS marquee, track duplicated for a seamless loop. */
export function Ticker() {
  return (
    <>
      <div className="overflow-hidden bg-ink py-2 text-parchment" aria-hidden>
        <div className="marquee">
          {[0, 1].map((copy) => (
            <div key={copy} className="flex shrink-0">
              {ticker.map((t) => (
                <span
                  key={t}
                  className="flex items-center gap-6 pr-6 text-caption uppercase sm:gap-8 sm:pr-8"
                >
                  {t}
                  <span className="text-smoke">✳</span>
                </span>
              ))}
            </div>
          ))}
        </div>
      </div>
      <div className="band" aria-hidden />
    </>
  );
}

export function Nav() {
  return (
    <header className="sticky top-0 z-40 bg-parchment/90 backdrop-blur-md">
      <div className="mx-auto flex h-16 max-w-[1432px] items-center gap-4 px-5 sm:gap-6 sm:px-6">
        <a href="/" className="flex items-center gap-2.5">
          <Logo size={20} />
          <span className="font-serif text-body-lg">loom</span>
        </a>
        <nav className="ml-6 hidden items-center gap-6 lg:flex">
          {navLinks.map((l) => (
            <a
              key={l.label}
              href={l.href}
              className="text-body-sm uppercase transition-colors duration-150 hover:text-graphite"
            >
              {l.label}
            </a>
          ))}
        </nav>
        <div className="ml-auto flex items-center gap-3">
          <a
            href={REPO}
            className="btn-ghost hidden h-9 px-5 py-0 text-body-sm sm:inline-flex"
          >
            GitHub
          </a>
          <a
            href={WAITLIST}
            target="_blank"
            rel="noreferrer"
            className="btn-blue h-9 px-5 py-0 text-body-sm"
          >
            Join waitlist <span className="arrow">▸</span>
          </a>
        </div>
      </div>
    </header>
  );
}

function Column({
  title,
  links,
}: {
  title: string;
  links: { href: string; label: string }[];
}) {
  return (
    <div>
      <h4 className="font-serif text-body-lg">{title}</h4>
      <ul className="mt-5 space-y-3">
        {links.map((l) => (
          <li key={l.label}>
            <a
              href={l.href}
              target={l.href.startsWith("http") ? "_blank" : undefined}
              rel={l.href.startsWith("http") ? "noreferrer" : undefined}
              className="text-body-sm text-graphite transition-colors duration-150 hover:text-off-black"
            >
              {l.label}
            </a>
          </li>
        ))}
      </ul>
    </div>
  );
}

export function Footer() {
  return (
    <>
      <div className="band" aria-hidden />
      <footer className="mx-auto max-w-[1432px] px-5 py-14 sm:px-6 sm:py-20">
        <div className="grid gap-10 sm:grid-cols-2 lg:grid-cols-4">
          <div className="lg:pr-10">
            <span className="flex items-center gap-3">
              <Logo size={22} />
              <span className="font-serif text-subheading">loom</span>
            </span>
            <p className="mt-4 max-w-xs text-body-sm text-graphite">
              A coding agent with an IDE around it. One Rust binary, your
              folder, your keys.
            </p>
            <a
              href={WAITLIST}
              target="_blank"
              rel="noreferrer"
              className="btn-blue mt-6 h-9 px-4 py-0 text-caption"
            >
              Join waitlist <span className="arrow">▸</span>
            </a>
          </div>
          <Column title="Product" links={navLinks} />
          <Column title="Project" links={project} />
          <Column title="Connect" links={connect} />
        </div>

        <div className="mt-14 flex flex-wrap items-center gap-x-6 gap-y-3 border-t border-ash pt-8 text-caption uppercase text-smoke">
          <span>© 2026 Loom · PolyForm Noncommercial</span>
          <span>Windows · early build</span>
          <span className="sm:ml-auto">built in the open</span>
        </div>
      </footer>
    </>
  );
}

/** Shared shell for the legal pages — same chrome, editorial measure. */
export function Legal({
  title,
  updated,
  children,
}: {
  title: string;
  updated: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <Ticker />
      <Nav />
      <main className="relative isolate mx-auto max-w-3xl px-5 py-16 sm:px-6 sm:py-24">
        <div
          aria-hidden
          className="pointer-events-none absolute top-[-160px] right-[-40px] -z-10 h-[280px] w-[320px] rounded-full bg-linear-to-bl from-gold/80 to-coral/70 opacity-35 blur-[75px]"
        />
        <p className="text-caption uppercase text-smoke">
          <a href="/" className="text-off-black">
            loom
          </a>{" "}
          — legal
        </p>
        <h1 className="mt-5 font-serif text-heading-sm sm:text-heading-lg">
          {title}
        </h1>
        <p className="mt-4 text-caption uppercase text-smoke">
          Last updated {updated}
        </p>
        <div className="legal mt-12">{children}</div>
        <a href="/" className="btn-ghost mt-14">
          ← Back to the site
        </a>
      </main>
      <Footer />
    </>
  );
}
