"use client";

import { useState } from "react";
import { IconChevron, IconClose, IconGear } from "@/components/Icons";
import type { ModelCatalog, ProviderGroup, ProviderPatch } from "@/lib/api";
import { api } from "@/lib/api";

/**
 * Provider settings, the UI counterpart of opencode's auth flow: paste an API
 * key per provider (or point openai-compatible ones at another base URL) and
 * it is stored server-side in config.json. Keys are never shown again after
 * saving — only a status dot.
 */
export function SettingsModal({
  catalog,
  onClose,
  onSaved,
}: {
  catalog: ModelCatalog | null;
  onClose: () => void;
  onSaved: (c: ModelCatalog) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, ProviderPatch>>({});
  const [openId, setOpenId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const groups: ProviderGroup[] = catalog?.groups ?? [];

  function patch(id: string, next: Partial<ProviderPatch>) {
    setDrafts((prev) => ({ ...prev, [id]: { ...prev[id], ...next } }));
  }

  async function save(id: string, body: ProviderPatch) {
    setBusy(true);
    setErr("");
    try {
      const next = await api.saveProvider(id, body);
      onSaved(next);
      setDrafts((prev) => ({ ...prev, [id]: {} }));
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }

  async function clear(id: string) {
    await save(id, { clear: true });
  }

  return (
    <div className="modal-backdrop" onClick={onClose} role="dialog" aria-modal="true" aria-label="Provider settings">
      <div className="settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-head">
          <span className="settings-title">
            <IconGear />
            Providers
          </span>
          <button className="icon-btn" onClick={onClose} title="Close settings">
            <IconClose />
          </button>
        </div>
        <p className="settings-sub">
          Keys are saved to your user profile (config.json) and never shown again. Env vars still
          work as fallbacks, and models stay greyed until their provider has a key.
        </p>
        {/* One line per provider, opened only while you are editing it. The flat
            version stacked two inputs, two buttons and a repeated hint for every
            provider, so the list ran several screens for a single edit. */}
        <div className="settings-list">
          {groups.map((g) => {
            const draft = drafts[g.id] ?? {};
            const ready = g.key_set || g.key_optional;
            const showUrl = g.kind === "openai";
            const open = openId === g.id;
            const status = g.key_optional ? "no key needed" : g.key_set ? "connected" : "needs key";
            return (
              <div key={g.id} className={`settings-row ${ready ? "" : "off"} ${open ? "open" : ""}`}>
                <button
                  className="settings-row-head"
                  onClick={() => setOpenId(open ? null : g.id)}
                  aria-expanded={open}
                  title={g.key_optional ? undefined : `Env: ${g.env_keys.join(" or ")}`}
                >
                  <span className={`picker-dot ${ready ? "ok" : "missing"}`} />
                  <span className="settings-name">{g.label}</span>
                  <span className={`settings-status ${ready ? "" : "want"}`}>{status}</span>
                  <IconChevron className={`settings-chev ${open ? "down" : ""}`} />
                </button>

                {open && (
                  <div className="settings-body">
                    {!g.key_optional && (
                      <input
                        type="password"
                        placeholder={g.key_set ? "Replace API key…" : g.env_keys.join(" or ")}
                        value={draft.api_key ?? ""}
                        onChange={(e) => patch(g.id, { api_key: e.target.value })}
                        onKeyDown={(e) => {
                          if (e.key === "Enter" && (draft.api_key || draft.base_url)) {
                            save(g.id, draft);
                          }
                        }}
                        autoComplete="off"
                        spellCheck={false}
                        autoFocus
                      />
                    )}
                    {showUrl && (
                      <input
                        type="text"
                        placeholder={g.default_base_url}
                        value={draft.base_url ?? ""}
                        onChange={(e) => patch(g.id, { base_url: e.target.value })}
                        spellCheck={false}
                      />
                    )}
                    <div className="settings-actions">
                      <button
                        className="btn btn-sm btn-primary"
                        disabled={busy || (!draft.api_key && !draft.base_url)}
                        onClick={() => save(g.id, draft)}
                      >
                        {busy ? "Saving…" : "Save"}
                      </button>
                      {(g.key_set || showUrl) && (
                        <button
                          className="btn btn-sm ghost"
                          disabled={busy}
                          onClick={() => clear(g.id)}
                        >
                          Clear
                        </button>
                      )}
                    </div>
                  </div>
                )}
              </div>
            );
          })}
          {groups.length === 0 && <div className="settings-hint">Could not load providers.</div>}
        </div>
        {err && <div className="welcome-err">{err}</div>}
      </div>
    </div>
  );
}
