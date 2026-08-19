"use client";

import { IconCheck, IconClip } from "@/components/Icons";
import { useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Agent replies arrive as markdown; render it instead of showing the raw source. */
export function Markdown({ text }: { text: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          code(props) {
            const { children, className } = props;
            // fenced blocks carry a language class, inline code does not
            return className?.startsWith("language-") ? (
              <CodeBlock text={String(children).replace(/\n$/, "")} />
            ) : (
              <code className="md-inline">{children}</code>
            );
          },
          a: (props) => <a {...props} target="_blank" rel="noreferrer" />,
        }}
      >
        {text}
      </ReactMarkdown>
    </div>
  );
}

function CodeBlock({ text }: { text: string }) {
  return (
    <div className="md-code">
      <CopyButton text={text} label="" />
      <pre>
        <code>{text}</code>
      </pre>
    </div>
  );
}

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setDone(true);
      setTimeout(() => setDone(false), 1500);
    } catch {
      setDone(false);
    }
  }

  return (
    <button className={`copy-btn ${label ? "" : "copy-float"}`} onClick={copy} title="Copy">
      {done ? <IconCheck /> : <IconClip />}
      {label && <span>{done ? "Copied" : label}</span>}
    </button>
  );
}
