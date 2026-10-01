"use client";

import type { ReactNode } from "react";

/** Opaque, scrollable shell for normal pages; the island stays transparent. */
export function PageBody({ children }: { children: ReactNode }) {
  return <div className="page-scroll">{children}</div>;
}
