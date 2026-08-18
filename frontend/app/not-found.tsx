import Link from "next/link";

export default function NotFound() {
  return (
    <div className="nf">
      <svg className="nf-mark" viewBox="0 0 16 16" aria-hidden>
        <circle cx="4" cy="4" r="2.1" />
        <circle cx="12" cy="4" r="2.1" />
        <circle cx="4" cy="12" r="2.1" />
        <circle cx="12" cy="12" r="2.1" />
      </svg>
      <h1>404</h1>
      <p>That page does not exist. It may have been moved, or the link is wrong.</p>
      <Link className="btn btn-primary" href="/">
        Back to workspace
      </Link>
    </div>
  );
}
