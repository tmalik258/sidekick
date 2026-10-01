// One shared cursor stream in window CSS pixels. Inside Tauri the island
// window is click-through most of the time and receives no mouse events, so
// Rust polls the global cursor and sends it here; DOM pointer events cover the
// moments the window is interactive and the plain-browser preview.

import { EVENTS, isTauri, listen } from "./bridge";

type Listener = (x: number, y: number) => void;

const listeners = new Set<Listener>();
let started = false;

function push(x: number, y: number) {
  for (const l of listeners) l(x, y);
}

function start() {
  if (started || typeof window === "undefined") return;
  started = true;
  window.addEventListener("pointermove", (e) => push(e.clientX, e.clientY), { passive: true });
  if (isTauri()) void listen(EVENTS.islandCursor, (p) => push(p.x, p.y));
}

export function subscribeCursor(listener: Listener): () => void {
  start();
  listeners.add(listener);
  return () => listeners.delete(listener);
}
