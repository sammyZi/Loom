"use client";

import {
  Archive,
  ArrowLeft,
  Check,
  ChevronRight,
  Copy,
  FileDiff,
  Folder,
  GitBranch,
  GitFork,
  Link2,
  Maximize2,
  MoreHorizontal,
  PanelLeft,
  Paperclip,
  Play,
  Plus,
  RefreshCw,
  Search,
  Server,
  Settings,
  SlidersHorizontal,
  Square,
  SquareTerminal,
  Trash2,
  Undo2,
  Volume2,
  VolumeX,
  X,
  type LucideProps,
} from "lucide-react";

// Thin wrappers so call sites stay stable and the app's CSS keeps sizing them
// (.icon-btn svg, .side-nav button svg, etc).
export const IconPanel = (p: LucideProps) => <PanelLeft {...p} />;
export const IconPlus = (p: LucideProps) => <Plus {...p} />;
export const IconFolder = (p: LucideProps) => <Folder {...p} />;
export const IconTerminal = (p: LucideProps) => <SquareTerminal {...p} />;
export const IconLink = (p: LucideProps) => <Link2 {...p} />;
export const IconCheck = (p: LucideProps) => <Check {...p} />;
export const IconClose = (p: LucideProps) => <X {...p} />;
export const IconBranch = (p: LucideProps) => <GitBranch {...p} />;
export const IconClone = (p: LucideProps) => <GitFork {...p} />;
export const IconSsh = (p: LucideProps) => <Server {...p} />;
export const IconDiff = (p: LucideProps) => <FileDiff {...p} />;
export const IconMaximize = (p: LucideProps) => <Maximize2 {...p} />;
export const IconRefresh = (p: LucideProps) => <RefreshCw {...p} />;
export const IconClip = (p: LucideProps) => <Paperclip {...p} />;
export const IconTrash = (p: LucideProps) => <Trash2 {...p} />;
export const IconChevron = (p: LucideProps) => <ChevronRight {...p} />;
export const IconCopy = (p: LucideProps) => <Copy {...p} />;
export const IconBack = (p: LucideProps) => <ArrowLeft {...p} />;
export const IconDots = (p: LucideProps) => <MoreHorizontal {...p} />;
export const IconSliders = (p: LucideProps) => <SlidersHorizontal {...p} />;
export const IconPlay = (p: LucideProps) => <Play {...p} />;
export const IconStop = (p: LucideProps) => <Square {...p} />;
export const IconSearch = (p: LucideProps) => <Search {...p} />;
export const IconArchive = (p: LucideProps) => <Archive {...p} />;
export const IconUndo = (p: LucideProps) => <Undo2 {...p} />;
export const IconGear = (p: LucideProps) => <Settings {...p} />;
export const IconSpeak = (p: LucideProps) => <Volume2 {...p} />;
export const IconSpeakOff = (p: LucideProps) => <VolumeX {...p} />;

/** App wordmark glyph — six petals around a centre, no lucide equivalent. */
export function IconMark({ className = "brand-mark" }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 16 16" aria-hidden>
      {[0, 60, 120, 180, 240, 300].map((deg) => (
        <path key={deg} d="M8 8A4 4 0 0 1 8 2 4 4 0 0 1 8 8Z" transform={`rotate(${deg} 8 8)`} />
      ))}
    </svg>
  );
}
