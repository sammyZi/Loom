export function apiBase(): string {
  if (typeof window === "undefined") return "http://127.0.0.1:8080";
  if (window.location.port === "3000") return "http://127.0.0.1:8080";
  return "";
}

export function wsBase(): string {
  if (typeof window === "undefined") return "ws://127.0.0.1:8080";
  const proto = window.location.protocol === "https:" ? "wss" : "ws";
  if (window.location.port === "3000") return `${proto}://127.0.0.1:8080`;
  return `${proto}://${window.location.host}`;
}

async function req(path: string, init?: RequestInit) {
  const r = await fetch(`${apiBase()}${path}`, {
    ...init,
    headers: { "content-type": "application/json", ...(init?.headers || {}) },
  });
  const text = await r.text();
  let body: unknown = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = { error: text };
  }
  if (!r.ok) {
    const err = (body as { error?: string })?.error || r.statusText;
    throw new Error(err);
  }
  return body;
}

export const api = {
  workspace: () => req("/workspace") as Promise<{ path: string | null }>,
  pick: () => req("/workspace/pick", { method: "POST" }) as Promise<{ path: string | null }>,
  open: (path: string) =>
    req("/workspace/open", { method: "POST", body: JSON.stringify({ path }) }) as Promise<{
      path: string;
    }>,
  tree: () => req("/files/tree"),
  content: (path: string) =>
    req(`/files/content?path=${encodeURIComponent(path)}`) as Promise<{
      path: string;
      content: string;
    }>,
  save: (path: string, content: string) =>
    req("/files/content", { method: "PUT", body: JSON.stringify({ path, content }) }),
  gitStatus: () =>
    req("/git/status") as Promise<{ branch: string; files: { path: string; status: string }[] }>,
  gitDiff: (path?: string) =>
    req(`/git/diff${path ? `?path=${encodeURIComponent(path)}` : ""}`) as Promise<{ diff: string }>,
  commit: (message: string) =>
    req("/git/commit", { method: "POST", body: JSON.stringify({ message }) }),
  runAgent: (prompt: string, model: string, mode: string, effort: string, sessionId?: string) =>
    req("/agent/run", {
      method: "POST",
      body: JSON.stringify({ prompt, model, mode, effort, session_id: sessionId }),
    }),
  /** Answer an approval request for a shell command (manual mode). */
  answerPermission: (id: string, allow: boolean) =>
    req("/agent/permission", {
      method: "POST",
      body: JSON.stringify({ id, allow }),
    }),
  cancelAgent: () => req("/agent/cancel", { method: "POST" }),
  shell: (cmd: string, id: string, background = false) =>
    req("/shell/run", {
      method: "POST",
      body: JSON.stringify({ cmd, id, background }),
    }) as Promise<
      | { exit_code: number; stdout: string; stderr: string }
      | { started: true; background: true; id: string }
    >,
  /** Kill whatever terminal `id` is running. No-op if it is idle. */
  cancelShell: (id: string) =>
    req("/shell/cancel", { method: "POST", body: JSON.stringify({ id }) }),
  models: () =>
    req("/agent/models") as Promise<{
      models: { id: string; label: string; hint: string }[];
      default: string;
    }>,
};

/** Archived chats, for the sidebar's archive view. */
export async function loadArchived(): Promise<SessionLite[]> {
  try {
    const { sessions } = (await req("/sessions/archived")) as { sessions: SessionLite[] };
    return sessions ?? [];
  } catch {
    return [];
  }
}

export async function unarchiveSession(id: string): Promise<void> {
  await req(`/sessions/${encodeURIComponent(id)}/unarchive`, { method: "POST" });
}

export type SessionLite = {
  id: string;
  folder: string;
  title: string;
  log: unknown[];
  at: number;
  created?: number;
  archived?: boolean;
};

/** Terminal output pushed over /ws/shell while a command is still running. */
export type ShellEvent = { type: "chunk"; id: string; text: string };

/**
 * Reconnecting WebSocket. The old sockets were created once and never
 * reopened, so a backend restart left a permanently dead feed/terminal.
 * Exponential backoff caps at 8s; `onStatus` reports connectivity so the UI
 * can show it. Malformed frames are dropped instead of crashing the handler.
 * Returns a dispose function that stops reconnection and closes the socket.
 */
export function connectWs(
  path: string,
  onMessage: (data: unknown) => void,
  onStatus?: (connected: boolean) => void,
): () => void {
  let sock: WebSocket | null = null;
  let disposed = false;
  let attempt = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const open = () => {
    if (disposed) return;
    sock = new WebSocket(`${wsBase()}${path}`);
    sock.onopen = () => {
      attempt = 0;
      onStatus?.(true);
    };
    sock.onmessage = (m) => {
      try {
        onMessage(JSON.parse(String(m.data)));
      } catch {
        /* not JSON — ignore */
      }
    };
    sock.onclose = () => {
      onStatus?.(false);
      if (disposed) return;
      attempt += 1;
      timer = setTimeout(open, Math.min(500 * 2 ** attempt, 8000));
    };
    // A failed connect only fires onerror; route it through close so the
    // backoff in onclose runs.
    sock.onerror = () => {
      try {
        sock?.close();
      } catch {
        /* already closing */
      }
    };
  };
  open();

  return () => {
    disposed = true;
    if (timer) clearTimeout(timer);
    sock?.close();
  };
}

export type FileNode = {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileNode[];
};

export type AgentEvent =
  | { type: "token"; text: string }
  | { type: "tool_call"; name: string; input: unknown }
  | { type: "tool_result"; name: string; output: string }
  | { type: "think"; text: string }
  | { type: "diff"; path: string; diff: string }
  | { type: "status"; message: string }
  /** Approval request for a shell command (manual mode). Answer via answerPermission. */
  | { type: "ask"; id: string; program: string; args: string }
  /** Approximate context-window usage for the session (chars vs budget). */
  | { type: "context"; used: number; limit: number }
  | { type: "usage"; tokens: number }
  | { type: "done"; summary: string }
  | { type: "error"; message: string };
