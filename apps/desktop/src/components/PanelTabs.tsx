"use client";

// The one tab row for the open island: Ask, Agents and Repos as words, with
// History and Settings as icons on the right. Ctrl Tab goes through all five.

import type { ReactNode } from "react";
import type { AskTab } from "@/lib/agents";
import { KeyHint } from "./ask/parts";
import { Icon } from "./Icon";
import { Tip } from "./Tip";

export type PanelTab = AskTab | "settings";

/** The Ctrl Tab order. Not a coder: no Agents or Repos. */
export function tabOrder(coder: boolean): PanelTab[] {
  return coder ? ["ask", "agents", "repos", "history", "settings"] : ["ask", "history", "settings"];
}

export function nextTab(current: PanelTab, coder: boolean, back: boolean): PanelTab {
  const order = tabOrder(coder);
  const i = Math.max(order.indexOf(current), 0);
  return order[(i + (back ? order.length - 1 : 1)) % order.length];
}

export function PanelTabs({
  current,
  onPick,
  coder,
  working = 0,
  alt = false,
  extra,
}: {
  current: PanelTab;
  onPick: (tab: PanelTab) => void;
  coder: boolean;
  working?: number;
  alt?: boolean;
  /** Shown just before the icons, like the One/Board switch. */
  extra?: ReactNode;
}) {
  const words = (
    [
      ["ask", "Ask"],
      ["agents", "Agents"],
      ["repos", "Repos"],
    ] as const
  ).filter(([id]) => coder || id === "ask");
  return (
    <div className="ak-tabs" role="tablist">
      {words.map(([id, label]) => (
        <button
          key={id}
          type="button"
          role="tab"
          aria-selected={current === id}
          onClick={() => onPick(id)}
          className="ak-tab chip"
        >
          {label}
          {id === "agents" && working > 0 && <i className="n not-italic">{working}</i>}
        </button>
      ))}
      <span className="ak-tabs-end">
        {extra}
        <span className="relative">
          <Tip label="History (Alt H)">
            <button
              type="button"
              role="tab"
              aria-selected={current === "history"}
              aria-label="History"
              onClick={() => onPick("history")}
              className="ak-tab ak-tabi chip"
            >
              <Icon name="history" size={14} />
            </button>
          </Tip>
          <KeyHint show={alt}>Alt H</KeyHint>
        </span>
        <Tip label="Settings">
          <button
            type="button"
            role="tab"
            aria-selected={current === "settings"}
            aria-label="Settings"
            onClick={() => onPick("settings")}
            className="ak-tab ak-tabi chip"
          >
            <Icon name="settings" size={14} />
          </button>
        </Tip>
      </span>
    </div>
  );
}
