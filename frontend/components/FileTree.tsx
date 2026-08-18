"use client";

import { type FileNode } from "@/lib/api";

export function FileTree({
  tree,
  active,
  onOpen,
}: {
  tree: FileNode | null;
  active: string | null;
  onOpen: (path: string) => void;
}) {
  if (!tree) return <div className="scroll" />;
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
  if (node.is_dir) {
    return (
      <li>
        {!root && <span>{node.name}/</span>}
        {node.children && (
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
      <button className={active === node.path ? "active" : ""} onClick={() => onOpen(node.path)}>
        {node.name}
      </button>
    </li>
  );
}
