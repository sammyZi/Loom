import { IconMark } from "@/components/Icons";
import Link from "next/link";

export default function NotFound() {
  return (
    <div className="nf">
      <IconMark className="nf-mark" />
      <h1>404</h1>
      <p>That page does not exist. It may have been moved, or the link is wrong.</p>
      <Link className="btn btn-primary" href="/">
        Back to workspace
      </Link>
    </div>
  );
}
