// Internet connection. Rust checks that the internet is actually reachable
// (being on Wi-Fi is not enough); Windows' own offline signal makes the page
// react the moment Wi-Fi or a cable drops, without waiting for that check.

import { api, EVENTS, listen } from "./bridge";
import { setOffline } from "./friendly";

/** A short note on the island when the connection drops or comes back. */
export interface NetNotice {
  online: boolean;
  title: string;
  detail: string;
}

/** How long each note stays open before the island goes small again. */
const LOST_SHOWN_MS = 6000;
const BACK_SHOWN_MS = 4000;

export function lostNotice(networkGone: boolean): NetNotice {
  return {
    online: false,
    title: networkGone ? "You're offline" : "No internet",
    detail: networkGone
      ? "Wi-Fi or cable disconnected. I'll let you know when it's back."
      : "Still on the network, but the internet isn't reachable. I'll keep checking.",
  };
}

export function backNotice(offlineMs: number): NetNotice {
  return { online: true, title: "Back online", detail: `You were offline for ${howLong(offlineMs)}.` };
}

export function howLong(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (min < 1) return "a few seconds";
  if (min < 60) return min === 1 ? "a minute" : `${min} minutes`;
  const h = Math.floor(min / 60);
  const rest = min % 60;
  return `${h} hr${rest ? ` ${rest} min` : ""}`;
}

interface NetState {
  online: boolean;
  offlineSince: number | null;
  netNotice: NetNotice | null;
}

/**
 * Follows the connection and keeps `online`, `offlineSince` and `netNotice`
 * up to date through `set`. Only the island passes `play`, so a sound
 * never plays twice.
 */
export function watchNet(
  get: () => NetState,
  set: (s: Partial<NetState>) => void,
  play: ((sound: "caution" | "transition_up") => void) | null,
  /** A happy face for a moment when the connection is back. */
  onBack?: () => void,
) {
  let hide: ReturnType<typeof setTimeout> | undefined;
  let disposed = false;

  const show = (notice: NetNotice, ms: number) => {
    clearTimeout(hide);
    set({ netNotice: notice });
    hide = setTimeout(() => set({ netNotice: null }), ms);
  };

  const apply = (online: boolean) => {
    if (disposed) return;
    const { online: was, offlineSince } = get();
    if (online === was) return;
    setOffline(!online);
    if (online) {
      set({ online: true, offlineSince: null });
      show(backNotice(Date.now() - (offlineSince ?? Date.now())), BACK_SHOWN_MS);
      play?.("transition_up");
      onBack?.();
    } else {
      set({ online: false, offlineSince: Date.now() });
      show(lostNotice(typeof navigator !== "undefined" && !navigator.onLine), LOST_SHOWN_MS);
      play?.("caution");
    }
  };

  // Windows says the network is gone: trust it now, then confirm.
  const onOffline = () => {
    apply(false);
    void api.netCheck(true).then(apply, () => {});
  };
  // Windows says a network is back: only the real check says the internet is.
  const onOnline = () => void api.netCheck(false).then(apply, () => {});

  window.addEventListener("offline", onOffline);
  window.addEventListener("online", onOnline);
  const off = listen(EVENTS.netStatus, apply);
  // Started offline: say so once, without a sound.
  void api.netStatus().then(
    (online) => {
      if (!online && !disposed && get().online) {
        setOffline(true);
        set({ online: false, offlineSince: Date.now() });
        show(lostNotice(!navigator.onLine), LOST_SHOWN_MS);
      }
    },
    () => {},
  );

  return () => {
    disposed = true;
    clearTimeout(hide);
    window.removeEventListener("offline", onOffline);
    window.removeEventListener("online", onOnline);
    void off.then((f) => f());
  };
}
