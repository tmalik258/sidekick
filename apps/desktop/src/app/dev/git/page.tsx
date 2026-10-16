"use client";

// Development page: the Git dock against the browser mock.

import { GitDock } from "@/components/agents/GitDock";

export default function GitDockDev() {
  return (
    <main className="mx-auto min-h-screen max-w-md bg-black p-6 text-white">
      <GitDock path="C:/code/app" onClose={() => undefined} />
    </main>
  );
}
