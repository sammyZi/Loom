import { Ticker, Nav, Footer, navLinks, REPO } from "@/components/chrome";

export default function NotFound() {
  return (
    <>
      <Ticker />
      <Nav />

      <main className="relative isolate mx-auto flex max-w-3xl flex-col items-center px-5 py-24 text-center sm:px-6 sm:py-36">
        <div
          aria-hidden
          className="pointer-events-none absolute top-[-120px] left-1/2 -z-10 h-[300px] w-[420px] -translate-x-1/2 rounded-full bg-linear-to-r from-coral/80 via-periwinkle-mist to-sky-blue/80 opacity-45 blur-[75px]"
        />

        <p className="text-caption uppercase text-smoke">error 404</p>
        <h1 className="mt-5 font-serif text-[64px] leading-none tracking-[-0.02em] sm:text-display">
          No such file
        </h1>
        <p className="mt-6 max-w-md text-body text-graphite sm:text-body-lg">
          The agent went looking for this page and came back with nothing. The
          rest of the site is still where you left it.
        </p>

        {/* a terminal line, in the app's own voice */}
        <div className="mt-10 w-full max-w-lg overflow-x-auto rounded-[28px] border border-ash p-6 text-left font-mono text-body-sm sm:p-8">
          <div>
            <span className="text-lake-blue">❯</span> open .{" "}
            <span className="text-smoke">--page</span>
          </div>
          <div className="mt-2 text-graphite">
            error: no such file or directory
          </div>
          <div className="mt-2 text-graphite">
            hint: try one of the panels below{" "}
            <span className="text-smoke">·</span> exit 1
          </div>
        </div>

        <div className="mt-10 flex flex-col items-center gap-3 sm:flex-row sm:gap-4">
          <a href="/" className="btn-blue w-full sm:w-auto">
            Back to the start <span className="arrow">▸</span>
          </a>
          <a href={REPO} className="btn-ghost w-full sm:w-auto">
            Read the source
          </a>
        </div>

        <nav className="mt-12 flex flex-wrap items-center justify-center gap-4">
          {navLinks.map((l) => (
            <a
              key={l.label}
              href={l.href}
              className="tag transition-colors duration-150 hover:border-off-black/30"
            >
              {l.label}
            </a>
          ))}
        </nav>
      </main>

      <Footer />
    </>
  );
}
