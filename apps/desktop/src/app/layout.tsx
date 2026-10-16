import type { Metadata } from "next";
import type { ReactNode } from "react";
import "./globals.css";
import "@/components/creature/creature.css";

export const metadata: Metadata = {
  title: "Duskie",
  description: "Proactive desktop companion",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body className="font-sans antialiased">{children}</body>
    </html>
  );
}
