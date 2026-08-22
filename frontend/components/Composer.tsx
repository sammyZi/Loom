"use client";

import { PromptInput } from "@/components/ui/ai-chat-input";
import type { ProviderGroup } from "@/lib/api";

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

/** Reasoning effort levels, mapped per provider on the backend. */
export const EFFORTS = ["Low", "Medium", "High"];

/**
 * Adapter around the shadcn PromptInput: keeps components/ui untouched and
 * translates its (value, meta) submit shape into the app's agent call.
 * The selected model is a full "provider/model" id owned by the app, not by
 * the input component.
 */
export function Composer({
  value,
  busy,
  onChange,
  onSubmit,
  onStop,
  groups,
  modelId,
  onModelChange,
  onOpenSettings,
}: {
  value: string;
  busy: boolean;
  onChange: (v: string) => void;
  onSubmit: (value: string, meta: SubmitMeta) => void;
  onStop: () => void;
  groups?: ProviderGroup[];
  modelId?: string;
  onModelChange?: (id: string) => void;
  onOpenSettings?: () => void;
}) {
  return (
    <div className="composer-shell">
      <PromptInput
        value={value}
        onChange={onChange}
        busy={busy}
        onStop={onStop}
        modes={MODES}
        efforts={EFFORTS}
        onSubmit={onSubmit}
        placeholder="Ask Loom to build features, fix bugs, or work on your code."
        groups={groups}
        modelId={modelId}
        onModelChange={onModelChange}
        onOpenSettings={onOpenSettings}
      />
    </div>
  );
}
