"use client";

import * as React from "react";
import { useRef, useState, useEffect, useCallback } from "react";
import {
  Bot,
  Box,
  Check,
  Code,
  Gauge,
  Hand,
  KeyRound,
  ListChecks,
  Search,
  Settings as SettingsIcon,
  ShieldCheck,
  Sparkles,
  Wand2,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { cn } from "@/lib/utils";
import type { ProviderGroup } from "@/lib/api";

// ----------------------------------------------------------------------
// Transition Physics
// ----------------------------------------------------------------------
const SPRING_TRANSITION = "max-width 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), height 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)";
const SMOOTH_HEIGHT_TRANSITION = "max-width 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), height 0.15s ease-out";

// ----------------------------------------------------------------------
// Types
// ----------------------------------------------------------------------
interface Attachment {
  id: string;
  file: File;
  url: string;
  name: string;
  width?: number;
  height?: number;
}

/** Friendly display name for a "provider/model" selection id. */
export function modelLabel(groups: ProviderGroup[] | undefined, id: string): string {
  if (!groups) return id.includes("/") ? id.split("/")[1] : id;
  for (const g of groups) {
    const m = g.models.find((m) => m.id === id);
    if (m) return m.label;
  }
  return id.includes("/") ? id.split("/").slice(1).join("/") : id;
}

function providerOf(id: string): string {
  return id.split("/")[0] ?? "";
}

// ----------------------------------------------------------------------
// Sub-components
// ----------------------------------------------------------------------
function MorphingText({ text }: { text: string }) {
  const [width, setWidth] = useState<number | "auto">("auto");
  const spanRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    if (spanRef.current) {
      setWidth(spanRef.current.offsetWidth);
    }
  }, [text]);

  return (
    <span
      className="relative inline-flex items-center justify-center overflow-hidden transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]"
      style={{ width }}
    >
      <span ref={spanRef} className="invisible whitespace-nowrap px-1">
        {text}
      </span>
      <span
        key={text}
        className="absolute inset-0 flex items-center justify-center whitespace-nowrap animate-in fade-in zoom-in-95 duration-300"
      >
        {text}
      </span>
    </span>
  );
}

function ModelIcon({ model, className }: { model: string; className?: string }) {
  const icons: Record<string, LucideIcon> = {
    deepseek: Zap,
    anthropic: Sparkles,
    openai: Bot,
    google: Sparkles,
    groq: Zap,
    xai: Bot,
    mistral: Code,
    openrouter: Wand2,
    together: Box,
    fireworks: Zap,
    cerebras: Zap,
    ollama: Box,
    lmstudio: Box,
  };
  const Icon = icons[providerOf(model)] ?? Bot;
  return <Icon className={cn("object-contain", className)} strokeWidth={1.75} aria-hidden />;
}

function ModeIcon({ mode, className }: { mode: string; className?: string }) {
  const icons: Record<string, LucideIcon> = {
    Auto: Wand2,
    Plan: ListChecks,
    Manual: Hand,
    Approve: ShieldCheck,
  };
  const Icon = icons[mode] ?? Gauge;
  return <Icon className={cn("object-contain", className)} strokeWidth={1.75} aria-hidden />;
}

function ArrowUpIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M7 12V2M7 2L2.5 6.5M7 2L11.5 6.5" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function MicIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <rect x="5" y="1" width="4" height="7" rx="2" stroke="currentColor" strokeWidth="1.5" />
      <path d="M2.75 6.5V7a4.25 4.25 0 0 0 8.5 0v-.5M7 11.25V13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function StopIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <rect x="3.5" y="3.5" width="7" height="7" rx="1.5" fill="currentColor" />
    </svg>
  );
}

function PlusIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M7 2.5V11.5M2.5 7H11.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="9" height="9" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <path d="M2.5 2.5L11.5 11.5M11.5 2.5L2.5 11.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}

function DynamicBarsIcon({ level }: { level: string }) {
  // DeepSeek reasoning_effort: low | medium | high.
  const rank: Record<string, number> = { Low: 1, Medium: 2, High: 3 };
  const bars = rank[level] ?? 1;
  const isMediumOrHigh = bars >= 2;
  const isHigh = bars >= 3;

  return (
    <svg width="14" height="14" viewBox="0 0 14 14" fill="none" aria-hidden="true">
      <rect x="1.5" y="8" width="2.5" height="4.5" rx="1" fill="currentColor" className="transition-opacity duration-300" opacity={1} />
      <rect x="5.75" y="5" width="2.5" height="7.5" rx="1" fill="currentColor" className="transition-opacity duration-300" opacity={isMediumOrHigh ? 1 : 0.3} />
      <rect x="10" y="2" width="2.5" height="10.5" rx="1" fill="currentColor" className="transition-opacity duration-300" opacity={isHigh ? 1 : 0.3} />
    </svg>
  );
}

// ----------------------------------------------------------------------
// Attachment Thumbnail
// ----------------------------------------------------------------------
function AttachmentThumb({
  attachment,
  index,
  onRemove,
  onOpen,
  registerRef,
}: {
  attachment: Attachment;
  index: number;
  onRemove: (id: string) => void;
  onOpen: (attachment: Attachment, rect: DOMRect) => void;
  registerRef: (id: string, el: HTMLButtonElement | null) => void;
}) {
  const [isHovered, setIsHovered] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);

  return (
    <button
      ref={(el) => {
        btnRef.current = el;
        registerRef(attachment.id, el);
      }}
      type="button"
      onMouseDown={(e) => e.preventDefault()}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      onClick={(e) => {
        e.stopPropagation();
        if (btnRef.current) {
          onOpen(attachment, btnRef.current.getBoundingClientRect());
        }
      }}
      style={{ animationDelay: `${index * 35}ms`, animationFillMode: "backwards" }}
      className={cn(
        "group relative size-12 shrink-0 overflow-hidden rounded-xl border border-border bg-muted outline-none",
        "transition-transform duration-200 ease-[cubic-bezier(0.175,0.885,0.32,1.275)] hover:scale-[1.04] active:scale-[0.96]",
        "animate-in fade-in slide-in-from-top-3 zoom-in-90 duration-400"
      )}
      aria-label={`Open preview of ${attachment.name}`}
    >
      <img src={attachment.url} alt={attachment.name} className="size-full object-cover" draggable={false} />
      <span className={cn("absolute inset-0 flex items-start justify-end bg-black/0 transition-colors duration-200", isHovered && "bg-black/25")}>
        <span
          role="button" tabIndex={-1}
          onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); }}
          onClick={(e) => { e.stopPropagation(); onRemove(attachment.id); }}
          className={cn(
            "m-1 flex size-4 items-center justify-center rounded-full bg-background/90 text-foreground/70 shadow-sm transition-all duration-200 ease-[cubic-bezier(0.175,0.885,0.32,1.275)] hover:bg-background hover:text-foreground hover:scale-110",
            isHovered ? "opacity-100 scale-100" : "opacity-0 scale-50 pointer-events-none"
          )}
          aria-label={`Remove ${attachment.name}`}
        >
          <CloseIcon />
        </span>
      </span>
    </button>
  );
}

// ----------------------------------------------------------------------
// Shared-Element Gallery Modal
// ----------------------------------------------------------------------
function AttachmentGalleryModal({
  attachment,
  originRect,
  onClose,
}: {
  attachment: Attachment;
  originRect: DOMRect;
  onClose: () => void;
}) {
  const [phase, setPhase] = useState<"opening" | "open" | "closing">("opening");
  const [targetRect, setTargetRect] = useState<{
    top: number;
    left: number;
    width: number;
    height: number;
    radius: number;
  } | null>(null);
  const imgRef = useRef<HTMLImageElement>(null);

  useEffect(() => {
    const maxW = Math.min(window.innerWidth * 0.86, 560);
    const maxH = Math.min(window.innerHeight * 0.78, 720);

    const naturalW = attachment.width || 800;
    const naturalH = attachment.height || 600;
    const scale = Math.min(maxW / naturalW, maxH / naturalH, 1.6);

    const width = naturalW * scale;
    const height = naturalH * scale;

    setTargetRect({
      top: (window.innerHeight - height) / 2,
      left: (window.innerWidth - width) / 2,
      width,
      height,
      radius: 20,
    });

    const raf = requestAnimationFrame(() => setPhase("open"));
    return () => cancelAnimationFrame(raf);
  }, [attachment]);

  const handleClose = useCallback(() => setPhase("closing"), []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") handleClose(); };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [handleClose]);

  const isOpen = phase === "open";
  const isClosing = phase === "closing";

  const geometry = isOpen && targetRect
      ? targetRect
      : { top: originRect.top, left: originRect.left, width: originRect.width, height: originRect.height, radius: 12 };

  const animEasing = isClosing ? "ease-out" : "cubic-bezier(0.175, 0.885, 0.32, 1.275)";
  const animDur = isClosing ? "0.3s" : "0.45s";
  const flipTransition = `top ${animDur} ${animEasing}, left ${animDur} ${animEasing}, width ${animDur} ${animEasing}, height ${animDur} ${animEasing}, border-radius ${animDur} ${animEasing}`;

  return (
    <div className="fixed inset-0 z-[100]" onClick={handleClose} role="dialog" aria-modal="true">
      <div className="absolute inset-0 bg-background/70 backdrop-blur-md transition-opacity duration-400" style={{ opacity: isOpen ? 1 : 0 }} />
      <div
        style={{
          position: "fixed",
          top: geometry.top, left: geometry.left, width: geometry.width, height: geometry.height,
          borderRadius: geometry.radius, transition: flipTransition, overflow: "hidden",
          boxShadow: isOpen ? "0 24px 60px -12px rgb(0 0 0 / 0.35)" : "0 0px 0px 0px rgb(0 0 0 / 0)",
        }}
        className="bg-muted"
        onTransitionEnd={() => { if (phase === "closing") onClose(); }}
        onClick={(e) => e.stopPropagation()}
      >
        <img ref={imgRef} src={attachment.url} alt={attachment.name} className="size-full object-cover" draggable={false} />
      </div>

      <button
        type="button" onClick={handleClose}
        style={{ opacity: isOpen ? 1 : 0, transform: isOpen ? "scale(1)" : "scale(0.7)" }}
        className={cn(
          "fixed right-4 top-4 flex size-9 items-center justify-center rounded-full bg-card/90 text-foreground/70 shadow-md backdrop-blur-sm",
          "transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)] hover:bg-card hover:text-foreground",
          !isOpen && "pointer-events-none"
        )}
      >
        <span className="scale-150"><CloseIcon /></span>
      </button>
    </div>
  );
}

// ----------------------------------------------------------------------
// Main Component
// ----------------------------------------------------------------------

export interface PromptInputProps {
  onSubmit?: (
    value: string,
    meta: { model: string; mode: string; effort: string; attachments: File[] }
  ) => void;
  placeholder?: string;
  className?: string;
  /** Provider catalog from GET /agent/models; sections per provider. */
  groups?: ProviderGroup[];
  /** Selected model as "provider/model". */
  modelId?: string;
  onModelChange?: (id: string) => void;
  onOpenSettings?: () => void;
  /** Run modes (Auto / Plan / Manual / Approve). */
  modes?: string[];
  /** Reasoning effort levels. */
  efforts?: string[];
  defaultValue?: string;
  value?: string;
  onChange?: (value: string) => void;
  maxAttachments?: number;
  /** The agent is running: the send button becomes Stop. */
  busy?: boolean;
  onStop?: () => void;
}

/**
 * Model picker grouped by provider, opencode-style. Unconfigured providers are
 * greyed with a hint instead of hidden, so users discover what exists; a
 * footer entry opens the settings modal where keys are entered.
 *
 * Search and arrow-key navigation are driven off one flattened list of the
 * selectable rows, so what the keyboard walks is exactly what is on screen.
 */
/** Most rows the picker draws at once; the rest are reached by searching. */
const MAX_ROWS = 60;

function ModelPicker({
  open,
  onOpen,
  onPick,
  onOpenSettings,
  groups,
  value,
}: {
  open: boolean;
  onOpen: () => void;
  onPick: (v: string) => void;
  onOpenSettings?: () => void;
  groups: ProviderGroup[] | undefined;
  value: string;
}) {
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const searchRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const q = query.trim().toLowerCase();
  // Flatten, then cap. A gateway reports 400+ models, and rendering every one
  // made the panel slow to open and impossible to read; search is the way
  // through a list that long, so the cap pushes you towards it.
  const hits: { g: ProviderGroup; m: ProviderGroup["models"][number]; ready: boolean }[] = [];
  let total = 0;
  for (const g of groups ?? []) {
    const ready = g.key_set || g.key_optional;
    for (const m of g.models) {
      if (
        q &&
        !m.label.toLowerCase().includes(q) &&
        !m.id.toLowerCase().includes(q) &&
        !g.label.toLowerCase().includes(q)
      ) {
        continue;
      }
      total += 1;
      if (hits.length < MAX_ROWS) hits.push({ g, m, ready });
    }
  }
  const capped = total > hits.length;

  // Only rows that can actually be chosen, in display order.
  const pickable = hits.filter((h) => h.ready).map((h) => h.m.id);

  // Reopening starts clean, with the cursor on what is currently selected.
  useEffect(() => {
    if (!open) return;
    setQuery("");
    const at = pickable.indexOf(value);
    setCursor(at >= 0 ? at : 0);
    const t = setTimeout(() => searchRef.current?.focus(), 40);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // A filter can shorten the list under the cursor; keep it in range.
  useEffect(() => {
    if (cursor > pickable.length - 1) setCursor(0);
  }, [pickable.length, cursor]);

  useEffect(() => {
    if (!open) return;
    listRef.current
      ?.querySelector('[data-cursor="1"]')
      ?.scrollIntoView({ block: "nearest" });
  }, [cursor, open, query]);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!pickable.length) return;
      const step = e.key === "ArrowDown" ? 1 : -1;
      setCursor((c) => (c + step + pickable.length) % pickable.length);
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const id = pickable[cursor];
      if (id) onPick(id);
    }
  };

  return (
    <div className="relative">
      <button
        type="button"
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          e.stopPropagation();
          onOpen();
        }}
        className={cn(
          "group flex items-center gap-1.5 rounded-full px-2.5 py-1.5 text-foreground/50 transition-all duration-200 outline-none hover:bg-accent/60 hover:text-foreground cursor-default",
          open && "bg-accent/60 text-foreground"
        )}
        aria-label={`Select model. Current: ${modelLabel(groups, value)}`}
      >
        <ModelIcon model={value} className="size-3.5 opacity-70 group-hover:opacity-100 transition-opacity" />
        <span className="text-xs font-semibold select-none">
          <MorphingText text={modelLabel(groups, value)} />
        </span>
      </button>

      <div
        style={{ transformOrigin: "bottom left" }}
        onKeyDown={onKeyDown}
        className={cn(
          "absolute bottom-full left-0 mb-2 z-50 w-80 rounded-xl border border-border bg-card/95 p-1 shadow-xl backdrop-blur-md flex flex-col transition-all duration-300 cursor-default",
          open
            ? "opacity-100 scale-100 translate-y-0 pointer-events-auto ease-[cubic-bezier(0.34,1.56,0.64,1)]"
            : "opacity-0 scale-95 translate-y-3 pointer-events-none"
        )}
      >
        {/* Search row like opencode's model picker: always present, so typing
            is the primary way through a long provider list. */}
        <div className="picker-search">
          <Search className="size-3.5 shrink-0 opacity-55" />
          <input
            ref={searchRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onClick={(e) => e.stopPropagation()}
            placeholder="Search models…"
            spellCheck={false}
            aria-label="Search models"
          />
        </div>

        <div
          ref={listRef}
          className="model-picker-scroll prompt-scrollbar flex flex-col gap-0.5 max-h-72 overflow-y-auto pr-0.5"
        >
          {hits.map(({ g, m, ready }, i) => {
            const selected = m.id === value;
            const atCursor = ready && pickable[cursor] === m.id;
            // One header per run of rows from the same provider. Driving it off
            // the flat list keeps headers correct once the list is capped.
            const head = i === 0 || hits[i - 1].g.id !== g.id;
            return (
              <div key={m.id} className="contents">
                {head && (
                  <div className="picker-group-head">
                    <span>{g.label}</span>
                    {ready ? (
                      <span className="picker-dot ok" />
                    ) : (
                      // Locked providers used to be inert, leaving no way
                      // forward from the picker. This jumps straight to keys.
                      <button
                        type="button"
                        className="picker-key"
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={(e) => {
                          e.stopPropagation();
                          onOpenSettings?.();
                        }}
                        title={`Add an API key (${g.env_keys.join(" or ")}) to use ${g.label}`}
                      >
                        <KeyRound className="size-2.5" />
                        Add key
                      </button>
                    )}
                  </div>
                )}
                <button
                  type="button"
                  disabled={!ready}
                  data-cursor={atCursor ? "1" : undefined}
                  onMouseDown={(e) => e.preventDefault()}
                  onMouseEnter={() => {
                    const at = pickable.indexOf(m.id);
                    if (at >= 0) setCursor(at);
                  }}
                  onClick={(e) => {
                    e.stopPropagation();
                    if (!ready) return;
                    onPick(m.id);
                  }}
                  title={
                    !ready
                      ? "Needs an API key — open settings"
                      : `${m.id}\n${m.hint ? `${m.hint} · ` : ""}${Math.round(m.context / 1000)}k context`
                  }
                  className={cn(
                    "group relative flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-xs font-medium outline-none transition-colors cursor-default",
                    atCursor && "bg-accent",
                    selected ? "text-foreground" : "text-foreground/70",
                    !ready && "opacity-45"
                  )}
                >
                  <ModelIcon model={m.id} className="size-3.5 opacity-85 shrink-0" />
                  <span className="truncate">{m.label}</span>
                  {/* Gateways carry near-duplicate names, so the real id is the
                      only way to tell two rows apart. */}
                  <span className="picker-id">{m.id.slice(g.id.length + 1)}</span>
                  {/* Kept mounted so the row width does not jump on select. */}
                  <Check
                    className={cn(
                      "size-3.5 shrink-0 text-[color:var(--accent-hi)]",
                      selected ? "opacity-100" : "opacity-0"
                    )}
                  />
                </button>
              </div>
            );
          })}
          {!groups && (
            <div className="px-2.5 py-3 text-xs text-muted-foreground">Loading models…</div>
          )}
          {groups && total === 0 && (
            <div className="px-2.5 py-3 text-xs text-muted-foreground">
              {query ? `No model matches “${query}”.` : "No models available."}
            </div>
          )}
          {capped && (
            <div className="picker-more">
              {hits.length} of {total} — keep typing to narrow
            </div>
          )}
        </div>
        <button
          type="button"
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            e.stopPropagation();
            onOpenSettings?.();
          }}
          className="mt-0.5 flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-xs font-medium text-foreground/60 outline-none hover:bg-accent hover:text-foreground cursor-default"
        >
          <SettingsIcon className="size-3.5 opacity-75" />
          Provider settings
        </button>
      </div>
    </div>
  );
}

/** Slim context-window usage indicator living in the composer's footer row. */

/** One pill + dropdown. Used for model, run mode and reasoning effort. */
function Picker({
  open,
  onOpen,
  onPick,
  value,
  options,
  renderIcon,
  label,
}: {
  open: boolean;
  onOpen: () => void;
  onPick: (v: string) => void;
  value: string;
  options: string[];
  renderIcon: (v: string, className?: string) => React.ReactNode;
  label: string;
}) {  return (
    <div className="relative">
      <button
        type="button"
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          e.stopPropagation();
          onOpen();
        }}
        className={cn(
          "group flex items-center gap-1.5 rounded-full px-2.5 py-1.5 text-foreground/50 transition-all duration-200 outline-none hover:bg-accent/60 hover:text-foreground cursor-default",
          open && "bg-accent/60 text-foreground"
        )}
        aria-label={`Select ${label}. Current: ${value}`}
      >
        {renderIcon(value, "size-3.5 opacity-70 group-hover:opacity-100 transition-opacity")}
        <span className="text-xs font-semibold select-none">
          <MorphingText text={value} />
        </span>
      </button>

      <div
        style={{ transformOrigin: "bottom left" }}
        className={cn(
          "absolute bottom-full left-0 mb-2.5 z-50 w-44 rounded-2xl border border-border bg-card/95 p-1 shadow-xl backdrop-blur-md flex flex-col gap-0.5 transition-all duration-300 cursor-default",
          open
            ? "opacity-100 scale-100 translate-y-0 pointer-events-auto ease-[cubic-bezier(0.34,1.56,0.64,1)]"
            : "opacity-0 scale-95 translate-y-3 pointer-events-none"
        )}
      >
        {options.map((opt) => (
          <button
            key={opt}
            type="button"
            onMouseDown={(e) => e.preventDefault()}
            onClick={(e) => {
              e.stopPropagation();
              onPick(opt);
            }}
            title={OPTION_HINTS[opt]}
            className={cn(
              "group relative flex h-7 w-full items-center gap-2 rounded-lg px-2 text-left text-xs font-medium outline-none transition-colors cursor-default hover:bg-accent",
              opt === value ? "text-foreground" : "text-foreground/70"
            )}
          >
            {renderIcon(opt, "size-3.5 opacity-85")}
            {opt}
          </button>
        ))}
      </div>
    </div>
  );
}

/**
 * Which modes can actually touch a terminal. "Approve" sounds like it prompts
 * for approval but in fact withholds the shell entirely, which is why the agent
 * would say it "can't launch processes" with no explanation.
 */
const OPTION_HINTS: Record<string, string> = {
  Auto: "Plans, writes and runs commands itself. Use this to start dev servers.",
  Plan: "Produces a plan only. Changes nothing, runs nothing.",
  Manual: "One pass, full tools. Asks you before each shell command.",
  Approve: "No shell at all — it lists the commands for you to run yourself.",
  Low: "Least reasoning, fastest, cheapest.",
  Medium: "Balanced reasoning.",
  High: "Most reasoning, slowest.",
};

export const PromptInput = React.forwardRef<HTMLDivElement, PromptInputProps>(
  (
    {
      onSubmit,
      placeholder = "Ask anything",
      className,
      groups,
      modelId,
      onModelChange,
      onOpenSettings,
      modes = ["Auto", "Plan", "Manual", "Approve"],
      efforts = ["Low", "Medium", "High"],
      defaultValue = "",
      value: controlledValue,
      onChange,
      maxAttachments = 6,
      busy = false,
      onStop,
    },
    ref
  ) => {
    const [expanded, setExpanded] = useState(false);
    const [isSmoothResize, setIsSmoothResize] = useState(false);
    const [localValue, setLocalValue] = useState(defaultValue);
    const selectedModel = modelId ?? "";
    const setSelectedModel = (id: string) => onModelChange?.(id);
    const [effortIndex, setEffortIndex] = useState(1);
    const [selectedMode, setSelectedMode] = useState(modes[0]);
    const [openPicker, setOpenPicker] = useState<null | "model" | "mode" | "effort">(null);

    const [attachments, setAttachments] = useState<Attachment[]>([]);
    const [activeAttachment, setActiveAttachment] = useState<{ attachment: Attachment; rect: DOMRect } | null>(null);

    // Audio/Voice recording states
    const [isRecording, setIsRecording] = useState(false);
    const [audioData, setAudioData] = useState<number[]>(new Array(5).fill(0));
    const valueRef = useRef(controlledValue !== undefined ? controlledValue : localValue);

    // Refs for Web Audio & Speech Recognition cleanup
    const streamRef = useRef<MediaStream | null>(null);
    const audioContextRef = useRef<AudioContext | null>(null);
    const rafRef = useRef<number | null>(null);
    const recognitionRef = useRef<any>(null);
    const demoIntervalRef = useRef<number | null>(null);
    const demoTextIntervalRef = useRef<number | null>(null);

    const [hoverStyle, setHoverStyle] = useState({ opacity: 0, transform: "translateY(0px) scale(0.95)", transition: "none" });
    const [containerHeight, setContainerHeight] = useState(116);
    const [textareaHeight, setTextareaHeight] = useState(68);
    const [isScrolling, setIsScrolling] = useState(false);

    const isControlled = controlledValue !== undefined;
    const value = isControlled ? controlledValue : localValue;
    const hasValue = value.trim() !== "" || attachments.length > 0;
    const hasAttachments = attachments.length > 0;

    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const internalContainerRef = useRef<HTMLDivElement>(null);
    const topFadeRef = useRef<HTMLDivElement>(null);
    const bottomFadeRef = useRef<HTMLDivElement>(null);
    const fileInputRef = useRef<HTMLInputElement>(null);
    const thumbRefs = useRef<Map<string, HTMLButtonElement | null>>(new Map());

    // Sync value ref for audio callback closure
    useEffect(() => {
      valueRef.current = value;
    }, [value]);

    const updateFades = () => {
      const el = textareaRef.current;
      if (!el) return;
      const { scrollTop, scrollHeight, clientHeight } = el;
      if (topFadeRef.current) {
        topFadeRef.current.style.opacity = Math.min(scrollTop / 20, 1).toString();
      }
      if (bottomFadeRef.current) {
        const bottomScroll = scrollHeight - clientHeight - scrollTop;
        bottomFadeRef.current.style.opacity = Math.min(Math.max(bottomScroll - 16, 0) / 10, 1).toString();
      }
    };

    const handleValueChange = useCallback((val: string) => {
      setIsSmoothResize(true);
      if (!isControlled) setLocalValue(val);
      onChange?.(val);
    }, [isControlled, onChange]);

    const expand = () => {
      setIsSmoothResize(false);
      setExpanded(true);
    };

    // --- Voice Recording Logic ---
    const stopRecording = useCallback(() => {
      if (recognitionRef.current) {
        recognitionRef.current.stop();
        recognitionRef.current = null;
      }
      if (rafRef.current) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      if (streamRef.current) {
        streamRef.current.getTracks().forEach((track) => track.stop());
        streamRef.current = null;
      }
      if (audioContextRef.current) {
        audioContextRef.current.close();
        audioContextRef.current = null;
      }
      if (demoIntervalRef.current) {
        window.clearInterval(demoIntervalRef.current);
        demoIntervalRef.current = null;
      }
      if (demoTextIntervalRef.current) {
        window.clearInterval(demoTextIntervalRef.current);
        demoTextIntervalRef.current = null;
      }
      setIsRecording(false);
      setAudioData(new Array(5).fill(0));
    }, []);

    const startRecording = useCallback(async () => {
      setIsSmoothResize(false);
      setExpanded(true);

      let stream: MediaStream | null = null;
      try {
        if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
          stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        }
      } catch (err) {
        console.warn("Microphone access denied or unavailable. Falling back to simulated voice mode for demo.");
      }

      setIsRecording(true);

      if (stream) {
        streamRef.current = stream;

        // Setup Web Audio API for visualizer
        const AudioCtx = window.AudioContext || (window as any).webkitAudioContext;
        const audioCtx = new AudioCtx();
        audioContextRef.current = audioCtx;

        const analyser = audioCtx.createAnalyser();
        analyser.fftSize = 64;
        const source = audioCtx.createMediaStreamSource(stream);
        source.connect(analyser);

        const dataArray = new Uint8Array(analyser.frequencyBinCount);

        const updateVisualizer = () => {
          analyser.getByteFrequencyData(dataArray);
          const bands = new Array(5).fill(0);
          const step = Math.floor(dataArray.length / 5);
          for (let i = 0; i < 5; i++) {
            let sum = 0;
            for (let j = 0; j < step; j++) {
              sum += dataArray[i * step + j];
            }
            bands[i] = sum / step / 255; // normalize to 0-1
          }
          setAudioData(bands);
          rafRef.current = requestAnimationFrame(updateVisualizer);
        };
        updateVisualizer();

        // Setup Speech Recognition
        const SpeechRecognition = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
        if (SpeechRecognition) {
          const recognition = new SpeechRecognition();
          recognition.continuous = true;
          recognition.interimResults = true;

          let baseline = valueRef.current;

          recognition.onresult = (event: any) => {
            let interimTranscript = "";
            let finalTranscript = "";

            for (let i = event.resultIndex; i < event.results.length; ++i) {
              if (event.results[i].isFinal) {
                finalTranscript += event.results[i][0].transcript;
              } else {
                interimTranscript += event.results[i][0].transcript;
              }
            }

            if (finalTranscript) {
               baseline += (baseline ? " " : "") + finalTranscript;
            }

            handleValueChange((baseline + (interimTranscript ? " " + interimTranscript : "")).trim());
          };

          recognition.onerror = (e: any) => {
            console.error("Speech recognition error", e);
            stopRecording();
          };

          recognition.onend = () => {
             stopRecording();
          };

          recognitionRef.current = recognition;
          recognition.start();
        } else {
          console.warn("Speech recognition is not available in this browser.");
          stopRecording();
        }
      } else {
        // No microphone: do not fake input, just end the attempt.
        stopRecording();
      }
    }, [handleValueChange, stopRecording]);

    // Keep textarea auto-scrolled to bottom while recording
    useEffect(() => {
      if (isRecording && textareaRef.current) {
        textareaRef.current.scrollTop = textareaRef.current.scrollHeight;
      }
    }, [value, isRecording]);

    // Ensure cleanup of mic/streams on unmount
    useEffect(() => {
      return () => {
        stopRecording();
        attachments.forEach((a) => URL.revokeObjectURL(a.url));
      };
    }, [stopRecording, attachments]);


    useEffect(() => {
      if ((value.trim() !== "" || hasAttachments) && !expanded) {
        setIsSmoothResize(false);
        setExpanded(true);
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [value, expanded, hasAttachments]);

    useEffect(() => {
      if (expanded && !isRecording) {
        const timer = setTimeout(() => {
          if (textareaRef.current) {
            textareaRef.current.focus();
            const length = textareaRef.current.value.length;
            textareaRef.current.setSelectionRange(length, length);
          }
        }, 50);
        return () => clearTimeout(timer);
      }
    }, [expanded, isRecording]);

    // ONLY updates height on value/text change. Adding attachments leaves this completely isolated.
    useEffect(() => {
      if (!textareaRef.current) return;
      const el = textareaRef.current;

      const currentHeight = el.style.height;
      el.style.transition = 'none';
      el.style.height = "0px";
      const scrollHeight = el.scrollHeight;
      el.style.height = currentHeight;
      void el.offsetHeight;
      el.style.transition = '';

      const newHeight = Math.max(68, Math.min(scrollHeight, 160));
      el.style.height = `${newHeight}px`;

      setTextareaHeight(newHeight);
      setIsScrolling(scrollHeight > 160);

      setTimeout(updateFades, 0);
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [value, expanded]);

    useEffect(() => {
      setContainerHeight(Math.max(124, textareaHeight + 56));
      setTimeout(updateFades, 0);
    }, [textareaHeight]);

    useEffect(() => {
      if (!openPicker) return;
      const handleOutsideClick = (e: MouseEvent) => {
        if (internalContainerRef.current && !internalContainerRef.current.contains(e.target as Node)) {
          setOpenPicker(null);
        }
      };
      document.addEventListener("mousedown", handleOutsideClick);
      return () => document.removeEventListener("mousedown", handleOutsideClick);
    }, [openPicker]);

    const handleBlur = (e: React.FocusEvent<HTMLDivElement>) => {
      if (internalContainerRef.current && internalContainerRef.current.contains(e.relatedTarget as Node)) return;
      if (value.trim() === "" && !hasAttachments && !isRecording) {
        setIsSmoothResize(false);
        setExpanded(false);
        setOpenPicker(null);
      }
    };

    const handleSubmit = () => {
      if (value.trim() === "" && !hasAttachments) return;
      setIsSmoothResize(false);
      onSubmit?.(value, {
        model: selectedModel,
        mode: selectedMode,
        effort: efforts[effortIndex],
        attachments: attachments.map((a) => a.file),
      });
      handleValueChange("");
      attachments.forEach((a) => URL.revokeObjectURL(a.url));
      setAttachments([]);
      setExpanded(false);
      setOpenPicker(null);
    };


    const openFileChooser = (e: React.MouseEvent) => {
      e.stopPropagation();
      fileInputRef.current?.click();
    };

    // Shared by the file picker and paste-an-image (Ctrl+V): both just hand
    // over a list of image files and want the same room check, expand, and
    // thumbnail sizing.
    const addFiles = (files: File[]) => {
      if (files.length === 0) return;
      const room = Math.max(0, maxAttachments - attachments.length);
      const accepted = files.slice(0, room);
      if (accepted.length === 0) return;

      if (!expanded) { setIsSmoothResize(false); setExpanded(true); }
      else { setIsSmoothResize(true); }

      for (const file of accepted) {
        const url = URL.createObjectURL(file);
        const img = new Image();
        img.onload = () => addAttachment(file, url, img.naturalWidth, img.naturalHeight);
        img.onerror = () => addAttachment(file, url, 800, 600);
        img.src = url;
      }
    };

    const handleFilesChosen = async (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = Array.from(e.target.files ?? []).filter((f) => f.type.startsWith("image/"));
      e.target.value = "";
      addFiles(files);
    };

    // Ctrl+V with an image on the clipboard (a screenshot, a copied image)
    // attaches it the same way the file picker does, instead of the browser's
    // default of doing nothing for image clipboard items in a plain textarea.
    const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const files = Array.from(e.clipboardData?.items ?? [])
        .filter((it) => it.kind === "file" && it.type.startsWith("image/"))
        .map((it) => it.getAsFile())
        .filter((f): f is File => f !== null);
      if (files.length === 0) return; // plain text paste: let the browser handle it
      e.preventDefault();
      addFiles(files);
    };

    const addAttachment = (file: File, url: string, width: number, height: number) => {
      const id = `${file.name}-${file.lastModified}-${Math.random().toString(36).slice(2, 8)}`;
      setAttachments((prev) => [...prev, { id, file, url, name: file.name, width, height }]);
    };

    const removeAttachment = (id: string) => {
      setIsSmoothResize(true);
      setAttachments((prev) => {
        const target = prev.find((a) => a.id === id);
        if (target) URL.revokeObjectURL(target.url);
        return prev.filter((a) => a.id !== id);
      });
      thumbRefs.current.delete(id);
    };

    // Calculate action button states
    const showStop = isRecording || busy;
    const showArrow = hasValue && !showStop;
    const showMic = !hasValue && !showStop;

    const onActionButtonClick = (e: React.MouseEvent) => {
      e.preventDefault();
      if (busy) {
        onStop?.();
      } else if (isRecording) {
        stopRecording();
      } else if (hasValue) {
        handleSubmit();
      } else {
        startRecording();
      }
    };

    return (
      <>
        {/* Outer Wrapper for positioning and max-width scaling */}
        <div
          ref={(node) => {
            if (typeof ref === "function") ref(node);
            else if (ref) ref.current = node;
            // @ts-ignore
            internalContainerRef.current = node;
          }}
          onBlur={handleBlur}
          className={cn("relative flex flex-col w-full", className)}
          style={{
            maxWidth: expanded ? 680 : 520,
            transition: isSmoothResize ? "max-width 0.15s ease-out" : "max-width 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)",
          }}
        >
          <input
            ref={fileInputRef}
            type="file"
            accept="image/*"
            multiple
            onChange={handleFilesChosen}
            className="hidden"
            tabIndex={-1}
            aria-hidden="true"
          />

          {/* Independent Attachment Tab (Slides up from behind the prompt input) */}
          <div
            aria-hidden={!hasAttachments}
            style={{
              height: hasAttachments && expanded ? 68 : 0,
              transition: isSmoothResize
                ? "height 0.15s ease-out"
                : "height 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)",
            }}
            className="w-full relative z-0 overflow-hidden"
          >
            <div
              style={{
                position: "absolute",
                bottom: -8,
                left: 20,
                right: 20,
                height: 68,
                transform: hasAttachments && expanded ? "translateY(0)" : "translateY(100%)",
                opacity: hasAttachments && expanded ? 1 : 0,
                transition: isSmoothResize
                  ? "transform 0.15s ease-out, opacity 0.15s ease-out"
                  : "transform 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275), opacity 0.3s ease-out",
              }}
              className="border border-border border-b-0 bg-muted rounded-t-2xl px-2 pt-2 pb-1 flex items-start gap-2 overflow-x-auto prompt-scrollbar"
            >
              {attachments.map((attachment, index) => (
                <AttachmentThumb
                  key={attachment.id}
                  attachment={attachment}
                  index={index}
                  onRemove={removeAttachment}
                  onOpen={(a, rect) => setActiveAttachment({ attachment: a, rect })}
                  registerRef={(id, el) => thumbRefs.current.set(id, el)}
                />
              ))}
            </div>
          </div>

          {/* Main Input Card */}
          <div
            onMouseDown={(e) => {
              if (!expanded || isRecording) return;
              if (e.target === textareaRef.current) return;
              // Clicking the card's empty space drops the caret in the
              // composer. Clicking a control that lives inside the card — the
              // model picker's search box above all — must keep its own focus,
              // or the preventDefault() below sends the typing here instead.
              const el = e.target as HTMLElement | null;
              if (
                el?.closest(
                  "input, textarea, select, [contenteditable=\"true\"], [role=\"dialog\"], [role=\"listbox\"], [role=\"menu\"], [role=\"combobox\"]",
                )
              ) {
                return;
              }
              e.preventDefault();
              textareaRef.current?.focus();
            }}
            style={{
              borderRadius: 24,
              height: expanded ? containerHeight : 52,
              transition: isSmoothResize ? SMOOTH_HEIGHT_TRANSITION : SPRING_TRANSITION,
              overflow: expanded ? "visible" : "hidden",
            }}
            className={cn(
              "relative w-full border border-border bg-card shadow-sm focus-within:border-ring/40 focus-within:ring-1 focus-within:ring-ring/20 hover:border-border/80 z-10",
              expanded ? "cursor-text" : "cursor-default"
            )}
          >
            <style dangerouslySetInnerHTML={{ __html: `
              .prompt-scrollbar::-webkit-scrollbar { width: 4px; height: 4px; background: transparent; }
              .prompt-scrollbar::-webkit-scrollbar-track { background: transparent; }
              .prompt-scrollbar::-webkit-scrollbar-thumb { background: transparent; border-radius: 4px; }
              .prompt-scrollbar:hover::-webkit-scrollbar-thumb { background: hsl(var(--muted-foreground) / 0.3); }
            `}} />

            <textarea
              ref={textareaRef}
              value={value}
              onChange={(e) => handleValueChange(e.target.value)}
              onScroll={updateFades}
              onPaste={handlePaste}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  handleSubmit();
                }
                if (e.key === "Escape" && value.trim() === "" && !hasAttachments) {
                  setIsSmoothResize(false);
                  setExpanded(false);
                  setOpenPicker(null);
                }
              }}
              placeholder={placeholder}
              aria-label="Prompt"
              disabled={isRecording}
              style={{
                transition: isSmoothResize
                  ? "height 0.15s ease-out"
                  : "opacity 0.3s ease-out, transform 0.3s ease-out, height 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)"
              }}
              className={cn(
                "prompt-scrollbar absolute top-0 inset-x-0 z-[1] w-full resize-none bg-transparent pl-5 pr-14 py-4 text-[15px] leading-[24px] text-foreground outline-none placeholder:font-medium placeholder:text-muted-foreground cursor-text",
                expanded ? "opacity-100 scale-100 translate-y-0" : "opacity-0 scale-95 -translate-y-1 pointer-events-none",
                isScrolling ? "overflow-y-auto" : "overflow-y-hidden",
                isRecording && "pointer-events-none"
              )}
            />

            <div
              ref={topFadeRef}
              className="absolute left-4 right-12 top-0 z-[2] h-8 bg-gradient-to-b from-card via-card/90 to-transparent pointer-events-none"
            />
            <div
              ref={bottomFadeRef}
              className="absolute left-4 right-12 z-[2] h-8 bg-gradient-to-t from-card via-card/90 to-transparent pointer-events-none"
              style={{
                opacity: 0,
                top: `${textareaHeight - 32}px`,
                transition: isSmoothResize ? "top 0.15s ease-out" : "top 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)"
              }}
            />

            <button
              type="button"
              onClick={expand}
              style={{ transition: isSmoothResize ? "none" : "all 0.4s cubic-bezier(0.175, 0.885, 0.32, 1.275)" }}
              className={cn(
                "absolute inset-x-0 top-0 z-[1] cursor-text truncate pl-5 pr-14 py-[16px] text-left text-[15px] font-medium leading-[20px] text-muted-foreground outline-none",
                !expanded ? "opacity-100 scale-100 translate-y-0" : "opacity-0 scale-105 translate-y-1 pointer-events-none"
              )}
              aria-label="Open prompt input"
            >
              {placeholder}
            </button>

            {/* Bottom Actions Wrapper - Hides when recording to make space for visualizer */}
            <div
              className={cn(
                "absolute bottom-2.5 left-4 right-14 z-[10] flex items-center gap-2 transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]",
                expanded && !isRecording ? "opacity-100 blur-0 translate-y-0 pointer-events-auto" : "opacity-0 blur-sm translate-y-2 pointer-events-none"
              )}
            >
              <ModelPicker
                open={openPicker === "model"}
                onOpen={() => setOpenPicker(openPicker === "model" ? null : "model")}
                onPick={(v) => {
                  setSelectedModel(v);
                  setOpenPicker(null);
                }}
                onOpenSettings={onOpenSettings}
                groups={groups}
                value={selectedModel}
              />

              {/* Context usage lives in the ContextBar above the composer now:
                  it is status, not an input control. */}
              <Picker
                open={openPicker === "mode"}
                onOpen={() => setOpenPicker(openPicker === "mode" ? null : "mode")}
                onPick={(v) => {
                  setSelectedMode(v);
                  setOpenPicker(null);
                }}
                value={selectedMode}
                options={modes}
                renderIcon={(v, c) => <ModeIcon mode={v} className={c} />}
                label="mode"
              />

              <Picker
                open={openPicker === "effort"}
                onOpen={() => setOpenPicker(openPicker === "effort" ? null : "effort")}
                onPick={(v) => {
                  setEffortIndex(Math.max(0, efforts.indexOf(v)));
                  setOpenPicker(null);
                }}
                value={efforts[effortIndex]}
                options={efforts}
                renderIcon={(v) => <DynamicBarsIcon level={v} />}
                label="reasoning effort"
              />

              <button
                type="button" onMouseDown={(e) => e.preventDefault()} onClick={openFileChooser} disabled={attachments.length >= maxAttachments}
                className="ml-auto flex size-7 items-center justify-center rounded-full text-foreground/50 transition-all duration-200 hover:bg-accent/60 hover:text-foreground outline-none cursor-default disabled:opacity-40 disabled:pointer-events-none"
              >
                <PlusIcon />
              </button>
            </div>

            {/* Audio Wave Visualizer Overlay positioned precisely to the left of the mic button */}
            <div
              className={cn(
                "absolute right-12 bottom-2 z-[10] flex h-8 items-center justify-end gap-[3px] transition-all duration-400 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]",
                isRecording ? "w-16 opacity-100 translate-x-0" : "w-0 opacity-0 translate-x-4 pointer-events-none"
              )}
            >
              {audioData.map((val, i) => (
                <div
                  key={i}
                  className="w-1 rounded-full bg-primary transition-[height] duration-75 ease-out"
                  style={{ height: `${Math.max(4, val * 24)}px` }}
                />
              ))}
            </div>

            <button
              type="button"
              onMouseDown={(e) => { e.preventDefault(); e.stopPropagation(); }}
              onClick={onActionButtonClick}
              aria-label={showArrow ? "Send prompt" : showStop ? "Stop" : "Use voice input"}
              style={{ borderRadius: 9999 }}
              className="absolute right-2.5 bottom-2.5 z-[10] flex h-8 w-8 items-center justify-center bg-primary text-primary-foreground transition-all duration-300 hover:opacity-90 outline-none focus-visible:ring-2 focus-visible:ring-ring cursor-default"
            >
              <span className="relative flex h-full w-full items-center justify-center">
                <span className={cn("absolute inset-0 flex items-center justify-center transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]", showArrow ? "opacity-100 scale-100 rotate-0 blur-none" : "opacity-0 scale-50 rotate-45 blur-[1px] pointer-events-none")}>
                  <ArrowUpIcon />
                </span>
                <span className={cn("absolute inset-0 flex items-center justify-center transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]", showMic ? "opacity-100 scale-100 rotate-0 blur-none" : "opacity-0 scale-50 -rotate-45 blur-[1px] pointer-events-none")}>
                  <MicIcon />
                </span>
                <span className={cn("absolute inset-0 flex items-center justify-center transition-all duration-300 ease-[cubic-bezier(0.175,0.885,0.32,1.275)]", showStop ? "opacity-100 scale-100 rotate-0 blur-none" : "opacity-0 scale-50 rotate-45 blur-[1px] pointer-events-none")}>
                  <StopIcon />
                </span>
              </span>
            </button>
          </div>
        </div>

        {activeAttachment && (
          <AttachmentGalleryModal
            attachment={activeAttachment.attachment} originRect={activeAttachment.rect} onClose={() => setActiveAttachment(null)}
          />
        )}
      </>
    );
  }
);

PromptInput.displayName = "PromptInput";
