"use client";

// Development page: the skills library against the browser mock.

import { SkillLibrary } from "@/components/settings/SkillLibrary";

export default function SkillsDev() {
  return (
    <main className="island-settings min-h-screen bg-black p-6">
      <div className="mx-auto max-w-xl">
        <SkillLibrary onError={(e) => console.error(e)} />
      </div>
    </main>
  );
}
