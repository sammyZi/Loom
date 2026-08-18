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
  runAgent: (prompt: string, model: string) =>
    req("/agent/run", { method: "POST", body: JSON.stringify({ prompt, model }) }),
  cancelAgent: () => req("/agent/cancel", { method: "POST" }),
  shell: (cmd: string) =>
    req("/shell/run", { method: "POST", body: JSON.stringify({ cmd }) }) as Promise<{
      exit_code: number;
      stdout: string;
      stderr: string;
    }>,
  models: () =>
    req("/agent/models") as Promise<{
      models: { id: string; label: string; hint: string }[];
      default: string;
    }>,
};

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
  | { type: "done"; summary: string }
  | { type: "error"; message: string };
