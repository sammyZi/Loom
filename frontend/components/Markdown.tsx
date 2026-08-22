"use client";

import { IconCheck, IconCopy, IconSpeak, IconSpeakOff } from "@/components/Icons";
import { useEffect, useState } from "react";
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
    <button
      className={`copy-btn ${done ? "ok" : ""} ${label ? "" : "copy-float"}`}
      onClick={copy}
      title="Copy to clipboard"
      aria-label="Copy to clipboard"
    >
      {/* Was a paperclip, which reads as "attach" rather than "copy". */}
      {done ? <IconCheck /> : <IconCopy />}
      {label && <span>{done ? "Copied" : label}</span>}
    </button>
  );
}

/**
 * Reads a reply aloud with the browser's own speech synthesis — no key, no
 * network, no dependency. Markdown is flattened first so the voice does not
 * spell out fences, hashes and link syntax.
 */
export function SpeakButton({ text, label = "Listen" }: { text: string; label?: string }) {
  const [speaking, setSpeaking] = useState(false);
  const supported = typeof window !== "undefined" && "speechSynthesis" in window;

  // Speech outlives the component, so a reply that scrolls away (or a reload)
  // would otherwise keep talking with no way to stop it.
  useEffect(() => () => window.speechSynthesis?.cancel(), []);

  if (!supported) return null;

  function toggle() {
    const synth = window.speechSynthesis;
    if (synth.speaking) {
      synth.cancel();
      setSpeaking(false);
      return;
    }
    const said = speakable(text);
    if (!said) return;
    setSpeaking(true);
    // One utterance per sentence: the default voice runs paragraphs together in
    // a flat rush, and the gap between utterances reads as natural punctuation.
    // It also keeps each chunk short, dodging the ~200-char cutoff some voices
    // hit on a single long utterance.
    const parts = said.match(/[^.!?]+[.!?]*\s*/g) ?? [said];
    const voice = humanVoice(synth);
    parts.forEach((part, i) => {
      const u = new SpeechSynthesisUtterance(part.trim());
      if (voice) u.voice = voice;
      u.rate = 1.02; // a shade above default; the default drags
      u.pitch = 1;
      u.volume = 1;
      if (i === parts.length - 1) {
        u.onend = () => setSpeaking(false);
      }
      u.onerror = () => setSpeaking(false);
      synth.speak(u);
    });
  }

  return (
    <button
      className={`copy-btn ${speaking ? "ok" : ""}`}
      onClick={toggle}
      title={speaking ? "Stop reading" : "Read this reply aloud"}
      aria-label={speaking ? "Stop reading" : "Read this reply aloud"}
    >
      {speaking ? <IconSpeakOff /> : <IconSpeak />}
      {label && <span>{speaking ? "Stop" : label}</span>}
    </button>
  );
}

/**
 * Prefer a natural-sounding local voice. Windows ships the flat "Microsoft
 * David/Zira" pair plus better "Online (Natural)" ones; browsers also expose
 * Google voices. Falls back to the default when none of them are installed.
 */
function humanVoice(synth: SpeechSynthesis): SpeechSynthesisVoice | null {
  const voices = synth.getVoices().filter((v) => v.lang.toLowerCase().startsWith("en"));
  if (!voices.length) return null;
  const rank = (v: SpeechSynthesisVoice) => {
    const n = v.name.toLowerCase();
    if (n.includes("natural")) return 0;
    if (n.includes("google")) return 1;
    if (n.includes("aria") || n.includes("jenny") || n.includes("guy")) return 2;
    if (v.localService) return 3;
    return 4;
  };
  return [...voices].sort((a, b) => rank(a) - rank(b))[0] ?? null;
}

/** Markdown flattened to something worth hearing. */
export function speakable(md: string): string {
  const src = md.trim();
  // Guard the blank case: the paragraph rule below would turn pure whitespace
  // into a lone ".", and a stripped leading image does the same.
  if (!src) return "";
  return src
    .replace(/```[\s\S]*?```/g, " code block. ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/^\s{0,3}#{1,6}\s+/gm, "")
    .replace(/^\s{0,3}>\s?/gm, "")
    .replace(/(\*\*|__|\*|_|~~)/g, "")
    .replace(/^\s*[-*+]\s+/gm, "")
    .replace(/\|/g, " ")
    .replace(/\n{2,}/g, ". ")
    .replace(/\s+/g, " ")
    .replace(/^[.\s]+/, "")
    .trim();
}
