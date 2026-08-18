"use client";

export function DiffView({ diff }: { diff: string }) {
  if (!diff) {
    return <pre className="diff" style={{ color: "var(--muted)" }}>No diff</pre>;
  }
  return (
    <pre className="diff">
      {diff.split("\n").map((line, i) => {
        const cls = line.startsWith("+") && !line.startsWith("+++")
          ? "add"
          : line.startsWith("-") && !line.startsWith("---")
            ? "del"
            : "";
        return (
          <div key={i} className={cls}>
            {line || " "}
          </div>
        );
      })}
    </pre>
  );
}
