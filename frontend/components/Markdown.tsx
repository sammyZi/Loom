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

  async function toggle() {
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
    // getVoices() returns [] until the browser finishes loading its voice
    // list, which had not happened yet on a first click — humanVoice() saw
    // nothing to rank, u.voice stayed unset, and the engine's own fallback
    // (Microsoft David on Windows) is the flattest voice available.
    const voice = humanVoice(await voicesReady(synth));
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
 * The list is empty until the engine finishes loading it — on Windows/Edge
 * that happens asynchronously, so a voice picked on the very first call saw
 * nothing and left the engine to its own flat default. Resolve once it is
 * populated, or after a short wait if `voiceschanged` never fires.
 */
function voicesReady(synth: SpeechSynthesis): Promise<SpeechSynthesisVoice[]> {
  const now = synth.getVoices();
  if (now.length) return Promise.resolve(now);
  return new Promise((resolve) => {
    const done = () => resolve(synth.getVoices());
    synth.addEventListener("voiceschanged", done, { once: true });
    setTimeout(done, 300);
  });
}

/**
 * Known English voice names by gender. The Web Speech API exposes no gender
 * field at all — only a name, a lang and a localService flag — so matching the
 * shipped voice names is the only way to ask for a female voice. Anything not
 * listed is treated as unknown rather than guessed at, and still beats a name
 * known to be male.
 */
const FEMALE =
  /(female|aria|jenny|michelle|ana|sonia|libby|natasha|clara|emily|zira|hazel|eva|samantha|victoria|karen|moira|tessa|fiona|allison|ava|susan|zoe|serena|catherine|linda|heather|amber|ashley|cora|elizabeth|monica|nova|joanna|salli|kendra|kimberly|ivy)/;
const MALE =
  /(male|david|mark|guy|ryan|george|christopher|eric|brian|daniel|alex|fred|tom|oliver|william|liam|steffan|roger|sam|arthur|thomas|gordon|james|jason|nathan|aaron|matthew|joey|justin|kevin)/;

/**
 * Prefer a natural-sounding female voice. Windows ships the flat legacy SAPI
 * voices (David/Zira/Mark/Sam) alongside far better "Online (Natural)" ones;
 * browsers also expose Google voices. Gender is the first sort key and quality
 * the second, so a female voice wins even when a male one sounds slicker —
 * falling back through unknown to male only when nothing else is installed.
 */
function humanVoice(voices: SpeechSynthesisVoice[]): SpeechSynthesisVoice | null {
  const en = voices.filter((v) => v.lang.toLowerCase().startsWith("en"));
  if (!en.length) return null;

  // 0 female, 1 unknown, 2 male — "Google UK English Female" and the like
  // carry the word outright, which is why `female`/`male` are in the lists.
  const gender = (n: string) => (FEMALE.test(n) ? 0 : MALE.test(n) ? 2 : 1);

  // Within a gender, how good the engine sounds.
  const quality = (v: SpeechSynthesisVoice, n: string) => {
    if (n.includes("natural")) return 0;
    if (n.includes("google")) return 1;
    // The classic flat SAPI voices: always worse than an untested unknown.
    if (/(david|zira|mark|sam)/.test(n)) return 4;
    if (v.localService) return 2;
    return 3;
  };

  const score = (v: SpeechSynthesisVoice) => {
    const n = v.name.toLowerCase();
    return gender(n) * 10 + quality(v, n);
  };
  return [...en].sort((a, b) => score(a) - score(b))[0] ?? null;
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
