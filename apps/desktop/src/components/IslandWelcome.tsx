"use client";

// First run: short steps inside the island. What Duskie
// does and what stays private, then a live checklist of the AI, connections
// and tools to set up, each with the exact command to run, and a few extras.
// Finishing on Start marks onboarding done; the same checklist stays in
// Settings > Setup. The current step is saved in settings so a restart
// resumes where the user left off.

import { WelcomeCard } from "./welcome/WelcomeCard";

export function IslandWelcome() {
  return <WelcomeCard />;
}
