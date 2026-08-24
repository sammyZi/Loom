import type { Metadata, Viewport } from "next";
import { Newsreader, JetBrains_Mono } from "next/font/google";
import "./globals.css";

const newsreader = Newsreader({
  subsets: ["latin"],
  weight: "400",
  variable: "--font-newsreader",
});
const jetbrains = JetBrains_Mono({
  subsets: ["latin"],
  variable: "--font-jetbrains",
});

export const metadata: Metadata = {
  title: "Loom — a local coding agent with an IDE around it",
  description:
    "One Rust binary. It opens your folder, edits your files, runs your commands, and reports what changed — on your machine, with your keys.",
};

export const viewport: Viewport = {
  themeColor: "#f6f3f1",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en" className={`${newsreader.variable} ${jetbrains.variable}`}>
      <body className="bg-parchment font-mono text-body text-off-black antialiased">
        {children}
      </body>
    </html>
  );
}
