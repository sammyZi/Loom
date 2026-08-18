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
    return <div className="editor-empty">Select a file from the tree</div>;
  }
  return (
    <Editor
      height="100%"
      theme="vs-dark"
      path={path}
      language={lang(path)}
      value={value}
      onChange={(v) => onChange(v ?? "")}
      options={{
        minimap: { enabled: false },
        fontSize: 13,
        fontFamily: "JetBrains Mono, Consolas, ui-monospace, monospace",
        automaticLayout: true,
        padding: { top: 12 },
        scrollBeyondLastLine: false,
        renderLineHighlight: "gutter",
        overviewRulerLanes: 0,
        hideCursorInOverviewRuler: true,
        scrollbar: {
          verticalScrollbarSize: 8,
          horizontalScrollbarSize: 8,
          arrowSize: 0,
          useShadows: false,
        },
      }}
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
