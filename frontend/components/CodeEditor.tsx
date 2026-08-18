"use client";

import Editor from "@monaco-editor/react";

export function CodeEditor({
  path,
  value,
  onChange,
}: {
  path: string | null;
  value: string;
  onChange: (v: string) => void;
}) {
  if (!path) {
    return <div className="scroll" style={{ padding: 16, color: "var(--muted)" }}>Select a file</div>;
  }
  return (
    <Editor
      height="100%"
      theme="vs-dark"
      path={path}
      language={lang(path)}
      value={value}
      onChange={(v) => onChange(v ?? "")}
      options={{ minimap: { enabled: false }, fontSize: 13, automaticLayout: true }}
    />
  );
}

function lang(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase();
  const map: Record<string, string> = {
    rs: "rust",
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    json: "json",
    md: "markdown",
    toml: "ini",
    css: "css",
    html: "html",
    py: "python",
    go: "go",
  };
  return map[ext || ""] || "plaintext";
}
