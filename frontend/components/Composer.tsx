"use client";

import { PromptInput } from "@/components/ui/ai-chat-input";

/** Label shown in PromptInput -> model id the backend expects. */
export const MODEL_IDS: Record<string, string> = {
  Flash: "deepseek-v4-flash",
  Pro: "deepseek-v4-pro",
};

export type SubmitMeta = { model: string; mode: string; effort: string; attachments: File[] };

/**
 * Run modes, surfaced as their own dropdown in the composer.
 * These strings are parsed by orchestrator::Mode::parse on the backend.
 *   Auto    planner -> coder -> reviewer, agent may run commands
 *   Plan    planner only, nothing is edited
 *   Manual  one agent, one pass, no planner or reviewer
 *   Approve agent has no shell; it lists commands for you to run
 */
export const MODES = ["Auto", "Plan", "Manual", "Approve"];

/** DeepSeek V4 reasoning_effort levels, per the API docs. */
export const EFFORTS = ["Low", "Medium", "High"];

/**
 * Adapter around the shadcn PromptInput: keeps components/ui untouched and
 * translates its (value, meta) submit shape into the app's agent call.
 */
export function Composer({
  value,
  models,
  busy,
  onChange,
  onSubmit,
  onStop,
}: {
  value: string;
  models: string[];
  busy: boolean;
  onChange: (v: string) => void;
  onSubmit: (value: string, meta: SubmitMeta) => void;
  onStop: () => void;
}) {
  return (
    <div className="composer-shell">
      <PromptInput
        value={value}
        onChange={onChange}
        models={models}
        busy={busy}
        onStop={onStop}
        modes={MODES}
        efforts={EFFORTS}
        onSubmit={onSubmit}
        placeholder="Ask Loom to build features, fix bugs, or work on your code."
      />
    </div>
  );
}
