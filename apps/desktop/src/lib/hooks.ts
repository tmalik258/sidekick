"use client";

import { useEffect, useState } from "react";

/** Re-renders every `ms` so time-based labels (pause countdown) stay fresh. */
export function useNow(ms: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
}
