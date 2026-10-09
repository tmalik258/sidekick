"use client";

// The first welcome step: what Sidekick is, in three short points, then
// your name and whether you work with code (which sets the path ahead).

import { type ReactNode, useEffect, useState } from "react";
import { api } from "@/lib/bridge";
import { updateSettings, useAssistantName, useSidekick } from "@/lib/store";

export function WelcomeIntro() {
  const hotkey = useSidekick((s) => s.settings.paletteHotkey);
  const name = useAssistantName();
  const voiceStatus = useSidekick((s) => s.voiceStatus);
  const downloading = voiceStatus?.downloading ?? false;
  const missing = (voiceStatus?.missingBytes ?? 0) > 0;
  return (
    <div className="flex flex-col gap-3 text-[13.5px] leading-relaxed text-white/90">
      {(downloading || missing) && (
        <p className="rounded-2xl bg-white/[0.06] px-3.5 py-2.5 text-[12.5px] text-[rgb(235_235_245/0.7)]">
          {downloading
            ? "Getting my voice ready (speech models download once, about 205 MB)…"
            : "Speech models will download so I can talk with you."}
        </p>
      )}
      <ul className="flex flex-col gap-2">
        <Point title="Summon">
          <Kbd>{hotkey}</Kbd> or say "Hey {name}".
        </Point>
        <Point title="Local">What I see stays on this PC.</Point>
        <Point title="Consent">I act only when you say. Anything I make can be undone.</Point>
      </ul>
      <AboutYou />
    </div>
  );
}

/** Your name (offered from Windows) and "Do you work with code?". */
function AboutYou() {
  const userName = useSidekick((s) => s.settings.userName);
  const codes = useSidekick((s) => s.settings.codes);
  const [draft, setDraft] = useState(userName);
  useEffect(() => {
    if (userName) return setDraft(userName);
    void api.userGuessName().then(
      (guess) => {
        if (!guess) return;
        setDraft(guess);
        void updateSettings({ userName: guess });
      },
      () => {},
    );
  }, [userName]);
  const save = () => {
    const v = draft.trim();
    if (v !== userName) void updateSettings({ userName: v });
  };
  return (
    <div className="flex flex-col gap-2 rounded-2xl bg-white/6 px-3.5 py-3">
      <label className="flex items-center gap-3">
        <span className="w-32 shrink-0 font-medium text-white">Your name</span>
        <input
          value={draft}
          placeholder="What should I call you?"
          spellCheck={false}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={save}
          onKeyDown={(e) => e.key === "Enter" && save()}
          className="min-w-0 flex-1 rounded-lg bg-white/8 px-2.5 py-1.5 text-[13px] text-white outline-none placeholder:text-white/40 focus:ring-1 focus:ring-[#0a84ff]"
        />
      </label>
      <div className="flex items-center gap-3">
        <span className="w-32 shrink-0 font-medium text-white">Work with code?</span>
        <fieldset className="flex gap-1.5" aria-label="Do you work with code?">
          {(
            [
              [true, "Yes"],
              [false, "No"],
            ] as const
          ).map(([v, label]) => (
            <button
              key={label}
              type="button"
              aria-pressed={codes === v}
              onClick={() => void updateSettings({ codes: v })}
              className={`chip rounded-full px-3.5 py-1 text-[12.5px] font-medium ${
                codes === v ? "bg-white text-black" : "bg-white/12 text-white/90 hover:bg-white/20"
              }`}
            >
              {label}
            </button>
          ))}
        </fieldset>
      </div>
      <p className="text-[11.5px] text-[rgb(235_235_245/0.62)]">
        {codes === false
          ? "I'll keep setup simple and skip the developer tools."
          : codes
            ? "I'll show coding agents and developer tools too."
            : "This sets which steps you see. You can change it in Settings."}
      </p>
    </div>
  );
}

function Point({ title, children }: { title: string; children: ReactNode }) {
  return (
    <li className="rounded-2xl bg-white/6 px-3.5 py-2.5">
      <p className="font-medium text-white">{title}</p>
      <p className="text-[12.5px] text-[rgb(235_235_245/0.6)]">{children}</p>
    </li>
  );
}

function Kbd({ children }: { children: ReactNode }) {
  return <kbd className="rounded-md bg-white/15 px-1.5 py-0.5 font-sans text-[11.5px] text-white">{children}</kbd>;
}
