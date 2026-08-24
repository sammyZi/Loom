import type { Metadata } from "next";
import { Legal } from "@/components/chrome";

export const metadata: Metadata = {
  title: "Privacy Policy — Loom",
  description:
    "What the Loom app stores on your machine, what the website collects, and who else sees anything.",
};

export default function Privacy() {
  return (
    <Legal title="Privacy Policy" updated="24 August 2026">
      <p>
        Loom is a desktop application that runs on your machine. It has no
        backend, no account system and no telemetry, so most of what a privacy
        policy usually covers simply does not happen here. This page describes
        the parts that do.
      </p>

      <h2>The application</h2>
      <p>
        Everything Loom creates stays in your user profile: sessions and
        message history in a local SQLite database, settings and provider API
        keys in local configuration files, and the code it reads or writes in
        the folder you opened. None of it is transmitted to us, because there
        is no &ldquo;us&rdquo; to transmit it to — no server takes part in a
        session.
      </p>

      <h2>Model providers</h2>
      <p>
        When you send a task, the prompt, the relevant file contents and the
        tool results are sent to the model provider you configured, using your
        own API key. That provider&rsquo;s privacy policy and data-retention
        terms govern what happens to it from there. Choosing a provider is
        choosing whose terms apply, so it is worth reading them. Loom talks to
        no other network service unless a task explicitly uses web search or
        fetches a URL.
      </p>

      <h2>The website</h2>
      <p>
        This site is a static export served as plain files. It sets no cookies,
        runs no analytics and embeds no third-party trackers. Fonts are served
        from Google Fonts, which means your browser makes a request to
        Google&rsquo;s font hosts when a page loads.
      </p>

      <h2>The waitlist</h2>
      <p>
        The waitlist form is hosted by Tally. If you submit it, the email
        address and any answers you give are stored by Tally on our behalf and
        used for one purpose: sending you a beta invite and occasional notes
        about the beta. It is not sold, rented or shared, and every message
        includes a way out. Ask and the entry is deleted.
      </p>

      <h2>Your choices</h2>
      <p>
        To remove application data, delete the Loom folder in your user
        profile — that is the whole of it. To remove a waitlist entry, email{" "}
        <a href="mailto:bhingesamarth@gmail.com">bhingesamarth@gmail.com</a>{" "}
        and it will be deleted.
      </p>

      <h2>Changes</h2>
      <p>
        If this policy changes, the date at the top changes with it. Material
        changes will be noted on the site rather than slipped in quietly.
      </p>
    </Legal>
  );
}
