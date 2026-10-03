// How a tool call reads while it runs, for Ask's steps and the island pill.

/** What a tool call looks like while it runs. Sidekick's own tools by name. */
export const LOCAL_TOOLS: Record<string, string> = {
  search: "Searching your PC...",
  web_search: "Searching the web...",
  sidekick_web_search: "Searching the web...",
  read_page: "Reading the page...",
  sidekick_read_page: "Reading the page...",
  WebSearch: "Searching the web...",
  WebFetch: "Reading the page...",
  browser: "Working in your browser...",
  sidekick_browser: "Working in your browser...",
  app_action: "Preparing the change...",
  sidekick_app_action: "Preparing the change...",
  desktop: "Working in the app...",
  sidekick_desktop: "Working in the app...",
  apps: "Looking up apps...",
  sidekick_apps: "Looking up apps...",
  notifications: "Checking your notifications...",
  sidekick_notifications: "Checking your notifications...",
  pc_status: "Checking your PC...",
  pc_control: "Changing a setting...",
  windows: "Looking at your windows...",
  today: "Checking your day...",
  recent: "Looking at what just happened...",
  open: "Opening...",
};

export function toolStatus(name: string): string {
  return LOCAL_TOOLS[name] ?? `Reading with ${toolLabel(name)}...`;
}

/** `JIRA_SEARCH_ISSUES` reads as "Jira search issues". */
export function toolLabel(name: string) {
  const words = name
    .toLowerCase()
    .split(/[_\-\s]+/)
    .filter(Boolean);
  if (words.length === 0) return "a tool";
  const [first, ...rest] = words;
  return [first.charAt(0).toUpperCase() + first.slice(1), ...rest].join(" ");
}
