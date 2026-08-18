"use client";

import { IconClose, IconPlus } from "@/components/Icons";
import { api } from "@/lib/api";
import { errText } from "@/lib/log";
import { useEffect, useRef, useState } from "react";

type Term = { id: number; name: string; log: string };

let nextId = 1;

export function TerminalPanel({ onClose }: { onClose: () => void }) {
  const [terms, setTerms] = useState<Term[]>([{ id: 1, name: "Terminal 1", log: "" }]);
  const [active, setActive] = useState(1);
  const [cmd, setCmd] = useState("");
  const [busy, setBusy] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);

  const current = terms.find((t) => t.id === active) || terms[0];

  useEffect(() => {
    const el = logRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [current?.log]);

  function add() {
    nextId += 1;
    const t = { id: nextId, name: `Terminal ${terms.length + 1}`, log: "" };
    setTerms([...terms, t]);
    setActive(t.id);
  }

  function closeTab() {
    if (terms.length <= 1) {
      onClose();
      return;
    }
    const next = terms.filter((t) => t.id !== active);
    setTerms(next);
    setActive(next[next.length - 1].id);
  }

  function append(text: string) {
    setTerms((prev) => prev.map((t) => (t.id === active ? { ...t, log: t.log + text } : t)));
  }

  async function run() {
    const line = cmd.trim();
    if (!line || busy) return;
    setCmd("");
    setBusy(true);
    append(`$ ${line}\n`);
    try {
      const r = await api.shell(line);
      append(`${r.stdout}${r.stderr}` || `(exit ${r.exit_code})\n`);
      if (r.exit_code !== 0) append(`exit ${r.exit_code}\n`);
    } catch (e) {
      append(`${errText(e)}\n`);
    }
    setBusy(false);
  }

  return (
    <div className="term">
      <div className="term-tabs">
        {terms.map((t) => (
          <button key={t.id} className={t.id === active ? "on" : ""} onClick={() => setActive(t.id)}>
            {t.name}
          </button>
        ))}
        <button className="icon-btn" onClick={add} title="New terminal">
          <IconPlus />
        </button>
        <span className="spacer" />
        <button className="icon-btn" onClick={closeTab} title="Close">
          <IconClose />
        </button>
      </div>
      <pre className="term-log" ref={logRef}>
        {current?.log || " "}
      </pre>
      <div className="term-in">
        <span>$</span>
        <input
          value={cmd}
          disabled={busy}
          onChange={(e) => setCmd(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") run();
          }}
          placeholder="command"
        />
      </div>
    </div>
  );
}
