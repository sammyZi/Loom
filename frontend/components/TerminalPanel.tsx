"use client";

import { IconClose, IconMaximize, IconPlus } from "@/components/Icons";
import { api } from "@/lib/api";
import { errText } from "@/lib/log";
import { useEffect, useRef, useState } from "react";

type Term = { id: number; name: string; log: string };

let nextId = 1;

/**
 * Lowest unused "Terminal N". Naming off the tab count reused numbers: with
 * tabs 1,2,3, closing 1 and 2 made the next tab collide with the surviving one.
 */
export function nextName(terms: { name: string }[]): string {
  const used = new Set(terms.map((t) => t.name));
  for (let i = 1; ; i++) {
    const name = `Terminal ${i}`;
    if (!used.has(name)) return name;
  }
}

export function TerminalPanel({
  cwd,
  maxed,
  onToggleMax,
  onClose,
}: {
  cwd: string;
  maxed: boolean;
  onToggleMax: () => void;
  onClose: () => void;
}) {
  const [terms, setTerms] = useState<Term[]>([{ id: 1, name: "Terminal 1", log: "" }]);
  const [active, setActive] = useState(1);
  const [cmd, setCmd] = useState("");
  const [busy, setBusy] = useState(false);
  const bodyRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const current = terms.find((t) => t.id === active) || terms[0];

  useEffect(() => {
    const el = bodyRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [current?.log, busy]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [active, maxed]);

  function add() {
    nextId += 1;
    const t = { id: nextId, name: nextName(terms), log: "" };
    setTerms([...terms, t]);
    setActive(t.id);
  }

  /**
   * Close one tab. Closing the last one also hides the panel and resets to a
   * single clean terminal, so reopening does not resurrect the closed one.
   */
  function closeTab(id: number) {
    const idx = terms.findIndex((t) => t.id === id);
    const next = terms.filter((t) => t.id !== id);
    if (next.length === 0) {
      nextId += 1;
      setTerms([{ id: nextId, name: "Terminal 1", log: "" }]);
      setActive(nextId);
      onClose();
      return;
    }
    setTerms(next);
    if (id === active) setActive(next[Math.min(idx, next.length - 1)].id);
  }

  function append(text: string) {
    setTerms((prev) => prev.map((t) => (t.id === active ? { ...t, log: t.log + text } : t)));
  }

  async function run() {
    const line = cmd.trim();
    if (!line || busy) return;
    setCmd("");
    setBusy(true);
    append(`${cwd}> ${line}\n`);
    try {
      const r = await api.shell(line);
      const out = `${r.stdout}${r.stderr}`;
      append(out || "");
      if (r.exit_code !== 0) append(`\nexit ${r.exit_code}\n`);
      if (out && !out.endsWith("\n")) append("\n");
    } catch (e) {
      append(`${errText(e)}\n`);
    }
    setBusy(false);
  }

  return (
    <div className="term">
      <div className="term-tabs">
        {terms.map((t) => (
          <div key={t.id} className={`term-tab ${t.id === active ? "on" : ""}`}>
            <button className="term-tab-name" onClick={() => setActive(t.id)}>
              {t.name}
            </button>
            <button
              className="term-tab-x"
              title={terms.length > 1 ? `Close ${t.name}` : "Hide terminal"}
              onClick={() => closeTab(t.id)}
            >
              <IconClose />
            </button>
          </div>
        ))}
        <button className="icon-btn term-add" onClick={add} title="New terminal">
          <IconPlus />
        </button>
        <span className="spacer" />
        <button
          className="icon-btn"
          onClick={onToggleMax}
          title={maxed ? "Restore panel" : "Maximize panel"}
        >
          <IconMaximize />
        </button>
        <button className="icon-btn" onClick={onClose} title="Hide terminal">
          <IconClose />
        </button>
      </div>

      <div className="term-body" ref={bodyRef} onClick={() => inputRef.current?.focus()}>
        {current?.log && <pre className="term-out">{current.log}</pre>}
        <div className="term-line">
          <span className="term-ps">{cwd}&gt;</span>
          {busy ? (
            <span className="term-running">running…</span>
          ) : (
            <input
              ref={inputRef}
              value={cmd}
              onChange={(e) => setCmd(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") run();
              }}
              spellCheck={false}
              autoComplete="off"
              aria-label="Terminal command"
            />
          )}
        </div>
      </div>
    </div>
  );
}
