"use client";

// Development page: the full editor against the browser mock.

import { Editor } from "@/components/agents/Editor";

export default function EditorDev() {
  return (
    <main className="flex h-screen bg-black p-6 text-white">
      <Editor root="C:/code/app" initial={{ path: "src/main.ts" }} onClose={() => undefined} />
    </main>
  );
}
