"use client";

// Grain wash behind the redesign island: holds the registry base tint and
// fires transient events (success, error, agent) as rings.

import { useEffect, useRef } from "react";
import type { WashEvent } from "@/lib/wash";
import { GrainWash, type GrainWashHandle } from "./GrainWash";

export function IslandGrain({
  base,
  event,
  className,
}: {
  base: WashEvent;
  /** Fires a ring when this changes to a non-idle event. */
  event?: WashEvent | null;
  className?: string;
}) {
  const handle = useRef<GrainWashHandle>(null);
  const last = useRef<WashEvent | null>(null);

  useEffect(() => {
    if (!event || event === last.current) return;
    last.current = event;
    if (event !== "idle") handle.current?.fire(event);
  }, [event]);

  return (
    <div className={className ?? "pointer-events-none absolute inset-0 overflow-hidden rounded-[inherit]"} aria-hidden>
      <GrainWash base={base} handle={handle} className="absolute inset-0 opacity-70" />
    </div>
  );
}
