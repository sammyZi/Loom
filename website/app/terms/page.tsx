import type { Metadata } from "next";
import { Legal, REPO } from "@/components/chrome";

export const metadata: Metadata = {
  title: "Terms — Loom",
  description:
    "The licence, what beta software means in practice, and the limits of what is promised.",
};

export default function Terms() {
  return (
    <Legal title="Terms & Legal" updated="24 August 2026">
      <p>
        Loom is source-available software, free for individuals and still in early
        development. These
        terms cover the software and this website; they are deliberately short,
        because there is not much to claim.
      </p>

      <h2>Licence</h2>
      <p>
        The source is published at{" "}
        <a href={REPO} target="_blank" rel="noreferrer">
          github.com/sammyZi/loom
        </a>{" "}
        under the PolyForm Noncommercial License 1.0.0. Use it, modify it, fork
        it and share it freely for anything that is not commercial — your own
        work, side projects, study, teaching, or a registered charity. Using it
        to produce work a company or client pays for is commercial use and needs
        a separate licence; email{" "}
        <a href="mailto:bhingesamarth@gmail.com">bhingesamarth@gmail.com</a>.
      </p>
      <p>
        Source-available is not the same as open source: the noncommercial
        restriction is a real one. The licence text in the repository is what
        governs; nothing on this page overrides it. Releases up to v0.1.0 went
        out under the MIT License and those grants stand for those versions.
      </p>

      <h2>Beta software</h2>
      <p>
        This is pre-release software. It can crash, it can misread a repository,
        and an agent acting on a bad instruction can edit files you would rather
        it left alone. Run it on work that is committed to version control, keep
        the permission rules tight on anything you cannot afford to lose, and
        review diffs before you stage them.
      </p>

      <h2>No warranty</h2>
      <p>
        The software is provided &ldquo;as is&rdquo;, without warranty of any
        kind, express or implied, including but not limited to the warranties of
        merchantability, fitness for a particular purpose and non-infringement.
        To the extent permitted by law, the authors are not liable for any
        claim, damages or other liability arising from the software or its use —
        including lost work, lost data or costs billed by a model provider.
      </p>

      <h2>Your responsibilities</h2>
      <p>
        You supply your own provider API keys and pay that provider directly.
        You are responsible for keeping those keys secure, for complying with
        the provider&rsquo;s terms, and for what the agent does with the
        permissions you grant it. Do not point it at systems you are not
        authorised to change.
      </p>

      <h2>The waitlist</h2>
      <p>
        A place on the waitlist is not a contract. Invites go out in batches, in
        no guaranteed order and on no guaranteed date, and the beta may change
        shape or pause without notice.
      </p>

      <h2>Contact</h2>
      <p>
        Questions, licence queries or security reports:{" "}
        <a href="mailto:bhingesamarth@gmail.com">bhingesamarth@gmail.com</a>.
      </p>
    </Legal>
  );
}
