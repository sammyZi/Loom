"use client";

import { type FileNode } from "@/lib/api";
import { useState } from "react";

export function FileTree({
  tree,
  active,
  onOpen,
}: {
  tree: FileNode | null;
  active: string | null;
  onOpen: (path: string) => void;
}) {
  if (!tree) return <div className="git-empty">No files</div>;
  return (
    <div className="scroll">
      <ul className="tree">
        <Node node={tree} active={active} onOpen={onOpen} root />
      </ul>
    </div>
  );
}

function Node({
  node,
  active,
  onOpen,
  root,
}: {
  node: FileNode;
  active: string | null;
  onOpen: (path: string) => void;
  root?: boolean;
}) {
  const [open, setOpen] = useState(true);
  if (node.is_dir) {
    return (
      <li>
        {!root && (
          <button className="row folder" onClick={() => setOpen((v) => !v)}>
            <span className="chev">{open ? "▾" : "▸"}</span>
            {node.name}
          </button>
        )}
        {open && node.children && (
          <ul>
            {node.children.map((c) => (
              <Node key={c.path || c.name} node={c} active={active} onOpen={onOpen} />
            ))}
          </ul>
        )}
      </li>
    );
  }
  return (
    <li>
      <button className={`row ${active === node.path ? "active" : ""}`} onClick={() => onOpen(node.path)}>
        <span className="chev" />
        {node.name}
      </button>
    </li>
  );
}
