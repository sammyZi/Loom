import type { ReactNode } from "react";

function S({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" aria-hidden>
      {children}
    </svg>
  );
}

export function IconMark() {
  return (
    <svg className="brand-mark" viewBox="0 0 16 16" aria-hidden>
      <circle cx="4" cy="4" r="2.1" />
      <circle cx="12" cy="4" r="2.1" />
      <circle cx="4" cy="12" r="2.1" />
      <circle cx="12" cy="12" r="2.1" />
    </svg>
  );
}

export function IconPanel() {
  return (
    <S>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M9 4v16" />
    </S>
  );
}

export function IconPlus() {
  return (
    <S>
      <path d="M12 5v14M5 12h14" />
    </S>
  );
}

export function IconFolder() {
  return (
    <S>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </S>
  );
}

export function IconTerminal() {
  return (
    <S>
      <path d="M5 8l3.5 4L5 16" />
      <path d="M12 16h7" />
    </S>
  );
}

export function IconLink() {
  return (
    <S>
      <path d="M10 13a5 5 0 0 0 7 0l2-2a5 5 0 0 0-7-7l-1 1" />
      <path d="M14 11a5 5 0 0 0-7 0l-2 2a5 5 0 0 0 7 7l1-1" />
    </S>
  );
}

export function IconCheck() {
  return (
    <S>
      <path d="M5 13l4 4L19 7" />
    </S>
  );
}

export function IconClose() {
  return (
    <S>
      <path d="M6 6l12 12M18 6L6 18" />
    </S>
  );
}

export function IconBranch() {
  return (
    <S>
      <circle cx="7" cy="6" r="2" />
      <circle cx="7" cy="18" r="2" />
      <circle cx="17" cy="9" r="2" />
      <path d="M7 8v8M9 9h4a2 2 0 0 1 2 2v0" />
    </S>
  );
}

export function IconClone() {
  return (
    <S>
      <path d="M12 4v10" />
      <path d="M8 10l4 4 4-4" />
      <path d="M5 18h14" />
    </S>
  );
}

export function IconSsh() {
  return (
    <S>
      <rect x="5" y="5" width="14" height="14" rx="2" />
      <path d="M12 8v5l3 2" />
    </S>
  );
}

export function IconDiff() {
  return (
    <S>
      <path d="M8 4v12M4 8h8" />
      <path d="M15 17h6" />
    </S>
  );
}

export function IconRefresh() {
  return (
    <S>
      <path d="M20 11a8 8 0 1 0-2.3 5.7" />
      <path d="M20 5v6h-6" />
    </S>
  );
}

export function IconClip() {
  return (
    <S>
      <path d="M20 11l-8.5 8.5a4 4 0 0 1-5.7-5.7l8.5-8.5a2.8 2.8 0 0 1 4 4l-8.5 8.5a1.6 1.6 0 0 1-2.2-2.2l7.8-7.8" />
    </S>
  );
}

export function IconTrash() {
  return (
    <S>
      <path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13" />
    </S>
  );
}
