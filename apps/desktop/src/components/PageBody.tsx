"use client";

import { type ReactNode, useEffect } from "react";

/** Gives normal pages an opaque, scrollable body; the island stays transparent. */
export function PageBody({ children }: { children: ReactNode }) {
  useEffect(() => {
    document.body.classList.add("page");
    return () => document.body.classList.remove("page");
  }, []);
  return children;
}
