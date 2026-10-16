// How a tool call reads while it runs, for Ask's steps and the island pill.

/** What a tool call looks like while it runs. Duskie's own tools by name. */
export const LOCAL_TOOLS: Record<string, string> = {
  search: "Searching your PC...",
  web_search: "Searching the web...",
  duskie_web_search: "Searching the web...",
  read_page: "Reading the page...",
  duskie_read_page: "Reading the page...",
  WebSearch: "Searching the web...",
  WebFetch: "Reading the page...",
  browser: "Working in your browser...",
  duskie_browser: "Working in your browser...",
  app_action: "Preparing the change...",
  duskie_app_action: "Preparing the change...",
  desktop: "Working in the app...",
  duskie_desktop: "Working in the app...",
  apps: "Looking up apps...",
  duskie_apps: "Looking up apps...",
  office: "Working in Office...",
  duskie_office: "Working in Office...",
  notifications: "Checking your notifications...",
  duskie_notifications: "Checking your notifications...",
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
