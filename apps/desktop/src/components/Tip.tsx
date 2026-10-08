"use client";

// In-webview tip. Native `title` tips are a separate Windows window and sit
// under the always-on-top island; this one portals into the same HWND.

import {
  cloneElement,
  isValidElement,
  type ReactElement,
  type ReactNode,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";

/** How long to wait before showing, like a system tip. */
const TIP_DELAY_MS = 400;
const TIP_GAP = 6;

export function Tip({
  label,
  children,
  className = "inline-flex min-w-0 max-w-full",
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  const id = useId();
  const wrapRef = useRef<HTMLSpanElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number; above: boolean } | null>(null);

  const clearTimer = () => {
    if (timer.current !== null) {
      clearTimeout(timer.current);
      timer.current = null;
    }
  };

  const hide = () => {
    clearTimer();
    setOpen(false);
    setPos(null);
  };

  const place = () => {
    const el = wrapRef.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    const spaceBelow = window.innerHeight - r.bottom;
    const above = spaceBelow < 96;
    setPos({
      left: Math.min(Math.max(r.left + r.width / 2, 12), window.innerWidth - 12),
      top: above ? r.top - TIP_GAP : r.bottom + TIP_GAP,
      above,
    });
  };

  const show = () => {
    clearTimer();
    timer.current = setTimeout(() => {
      place();
      setOpen(true);
    }, TIP_DELAY_MS);
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: runs once, on unmount
  useEffect(() => () => clearTimer(), []);

  if (!label) return children;

  const trigger = isValidElement(children)
    ? cloneElement(children as ReactElement<{ "aria-describedby"?: string }>, {
        "aria-describedby": open ? id : undefined,
      })
    : children;

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: hover and focus come from the wrapped control
    <span ref={wrapRef} className={className} onPointerEnter={show} onPointerLeave={hide} onFocus={show} onBlur={hide}>
      {trigger}
      {open &&
        pos &&
        createPortal(
          <span
            id={id}
            role="tooltip"
            className="ak-tip"
            data-above={pos.above || undefined}
            style={{ left: pos.left, top: pos.top }}
          >
            {label}
          </span>,
          document.body,
        )}
    </span>
  );
}
