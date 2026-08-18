"use client";

import { IconClip, IconClose } from "@/components/Icons";
import { useRef } from "react";

export const MODELS = [
  { id: "deepseek-v4-flash", label: "Flash" },
  { id: "deepseek-v4-pro", label: "Pro" },
] as const;

/** Text kept per attached file. Bigger files are truncated so the prompt stays sane. */
const MAX_CHARS = 200_000;

export type Attachment = { name: string; text: string; chars: number };

export function Composer({
  value,
  model,
  busy,
  attached,
  onChange,
  onModel,
  onAttach,
  onRemove,
  onRun,
  onStop,
}: {
  value: string;
  model: string;
  busy: boolean;
  attached: Attachment[];
  onChange: (v: string) => void;
  onModel: (id: string) => void;
  onAttach: (files: Attachment[]) => void;
  onRemove: (name: string) => void;
  onRun: () => void;
  onStop: () => void;
}) {
  const fileRef = useRef<HTMLInputElement>(null);

  async function pickFiles(list: FileList | null) {
    if (!list?.length) return;
    const out: Attachment[] = [];
    for (const f of Array.from(list)) {
      const raw = await f.text();
      if (raw.includes("\u0000")) continue; // skip binaries
      out.push({ name: f.name, text: raw.slice(0, MAX_CHARS), chars: raw.length });
    }
    if (out.length) onAttach(out);
    if (fileRef.current) fileRef.current.value = "";
  }

  return (
    <div className="composer">
      {attached.length > 0 && (
        <div className="attach-row">
          {attached.map((a) => (
            <span className="attach-chip" key={a.name}>
              {a.name}
              <em>{kb(a.chars)}</em>
              <button onClick={() => onRemove(a.name)} title="Remove">
                <IconClose />
              </button>
            </span>
          ))}
        </div>
      )}

      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="Ask ide-ai to build features, fix bugs, or work on your code."
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            onRun();
          }
        }}
      />

      <div className="composer-row">
        <input
          ref={fileRef}
          type="file"
          multiple
          hidden
          onChange={(e) => pickFiles(e.target.files)}
        />
        <button
          className="icon-btn"
          title="Attach files"
          disabled={busy}
          onClick={() => fileRef.current?.click()}
        >
          <IconClip />
        </button>
        <div className="seg">
          {MODELS.map((m) => (
            <button
              key={m.id}
              className={model === m.id ? "on" : ""}
              disabled={busy}
              onClick={() => onModel(m.id)}
            >
              {m.label}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <span className="hint">Shift+Enter for a new line</span>
        {busy ? (
          <button className="btn btn-danger" onClick={onStop}>
            Stop
          </button>
        ) : (
          <button className="btn btn-primary" disabled={!value.trim()} onClick={onRun}>
            Run
          </button>
        )}
      </div>
    </div>
  );
}

function kb(chars: number) {
  return chars < 1024 ? `${chars} B` : `${Math.round(chars / 1024)} KB`;
}
