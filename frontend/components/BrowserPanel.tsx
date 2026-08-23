"use client";

import {
  IconBack,
  IconClose,
  IconLink,
  IconMaximize,
  IconRefresh,
} from "@/components/Icons";
import { useEffect, useRef, useState } from "react";

/**
 * A deliberately light browser: one iframe plus a URL bar. No embedded engine,
 * no extra dependency — the host webview already has one, and a real engine
 * would dwarf the rest of the app.
 *
 * The trade-off that comes with an iframe is worth stating: cross-origin pages
 * cannot be read back, so the address bar tracks what we *asked* for rather
 * than where the page may have navigated itself, and sites sending
 * `X-Frame-Options: DENY` (Google, GitHub) refuse to load. Local dev servers —
 * the thing this exists for — have no such header.
 */
export function BrowserPanel({
  url,
  maxed,
  onToggleMax,
  onClose,
  onNavigate,
}: {
  /** Driven from outside so the agent can point it somewhere. */
  url: string;
  maxed: boolean;
  onToggleMax: () => void;
  onClose: () => void;
  onNavigate: (url: string) => void;
}) {
  const [draft, setDraft] = useState(url);
  // Bumping this remounts the iframe, which is the only reliable reload when
  // the page is cross-origin and contentWindow is off limits.
  const [nonce, setNonce] = useState(0);
  const [loading, setLoading] = useState(false);
  const back = useRef<string[]>([]);
  const forward = useRef<string[]>([]);

  useEffect(() => {
    setDraft(url);
    setLoading(Boolean(url));
  }, [url]);

  function go(next: string) {
    const target = normalizeUrl(next);
    if (!target || target === url) return;
    if (url) back.current.push(url);
    forward.current = [];
    onNavigate(target);
  }

  function goBack() {
    const prev = back.current.pop();
    if (!prev) return;
    forward.current.push(url);
    onNavigate(prev);
  }

  function goForward() {
    const next = forward.current.pop();
    if (!next) return;
    back.current.push(url);
    onNavigate(next);
  }

  return (
    <div className="term browser">
      <div className="term-tabs panel-head">
        <button
          className="icon-btn"
          onClick={goBack}
          disabled={back.current.length === 0}
          title="Back"
        >
          <IconBack />
        </button>
        <button
          className="icon-btn browser-fwd"
          onClick={goForward}
          disabled={forward.current.length === 0}
          title="Forward"
        >
          <IconBack />
        </button>
        <button
          className="icon-btn"
          onClick={() => {
            setLoading(true);
            setNonce((n) => n + 1);
          }}
          disabled={!url}
          title="Reload"
        >
          <IconRefresh />
        </button>
        <input
          className="browser-url"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") go(draft);
            if (e.key === "Escape") setDraft(url);
          }}
          placeholder="localhost:3000"
          spellCheck={false}
          aria-label="Address"
        />
        <button
          className="icon-btn"
          onClick={() => url && window.open(url, "_blank", "noreferrer")}
          disabled={!url}
          title="Open in your real browser"
        >
          <IconLink />
        </button>
        <button
          className="icon-btn"
          onClick={onToggleMax}
          title={maxed ? "Restore panel" : "Maximize panel"}
        >
          <IconMaximize />
        </button>
        <button className="icon-btn" onClick={onClose} title="Hide browser">
          <IconClose />
        </button>
      </div>

      <div className="browser-body">
        {url ? (
          <>
            {loading && <div className="browser-loading" aria-hidden />}
            <iframe
              key={`${url}#${nonce}`}
              src={url}
              title="Preview"
              onLoad={() => setLoading(false)}
              // Scripts and same-origin so a dev server's app actually runs;
              // top-navigation stays blocked so a page cannot hijack the app.
              sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
              referrerPolicy="no-referrer"
            />
          </>
        ) : (
          <div className="panel-empty">
            Type an address, or ask the agent to start the app — it opens the result here.
          </div>
        )}
      </div>
    </div>
  );
}

/** Accepts what people actually type: `localhost:3000`, `/about`, a full URL. */
export function normalizeUrl(input: string): string {
  const s = input.trim();
  if (!s) return "";
  if (/^https?:\/\//i.test(s)) return s;
  // A bare path is relative to the dev server people are usually looking at.
  if (s.startsWith("/")) return `http://localhost:3000${s}`;
  // Anything host-shaped defaults to http: these are local servers far more
  // often than public sites, and https on localhost is the rarer case.
  return `http://${s}`;
}
