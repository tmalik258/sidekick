"use client";

// The seam between the Repos list and the Git dock: drag it, nudge it with
// the arrow keys, or double click to reset. Neither side goes below its
// minimum, and the width is remembered.

import { useEffect, useRef } from "react";
import { clampIsland, defaultIsland, nudge, restore } from "@/lib/seam";

const KEY = "duskie.seam";

export function readSeam(total: number): number {
  let saved: number | null = null;
  try {
    const raw = localStorage.getItem(KEY);
    saved = raw ? Number(raw) : null;
  } catch {}
  return restore(Number.isFinite(saved) ? saved : null, total);
}

function save(w: number) {
  try {
    localStorage.setItem(KEY, String(w));
  } catch {}
}

export function Seam({ value, total, onChange }: { value: number; total: number; onChange: (w: number) => void }) {
  const drag = useRef<{ x: number; w: number } | null>(null);
  const set = (w: number) => {
    onChange(w);
    save(w);
  };
  useEffect(() => {
    const move = (e: PointerEvent) => {
      if (!drag.current) return;
      onChange(clampIsland(drag.current.w + e.clientX - drag.current.x, total));
    };
    const up = () => {
      if (!drag.current) return;
      drag.current = null;
      save(value);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
  }, [onChange, total, value]);
  return (
    // biome-ignore lint/a11y/useSemanticElements: a focusable, draggable separator has no native element
    <div
      className="gd-seam"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the Git dock"
      aria-valuenow={value}
      aria-valuemin={0}
      aria-valuemax={total}
      tabIndex={0}
      onPointerDown={(e) => {
        drag.current = { x: e.clientX, w: value };
        e.currentTarget.setPointerCapture?.(e.pointerId);
      }}
      onDoubleClick={() => set(defaultIsland(total))}
      onKeyDown={(e) => {
        const w = nudge(e.key, e.shiftKey, value, total);
        if (w === null) return;
        e.preventDefault();
        set(w);
      }}
    >
      <i />
    </div>
  );
}
