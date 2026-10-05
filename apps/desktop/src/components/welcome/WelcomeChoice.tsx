"use client";

// Toggle row shared by welcome Extras and FoundCard.

export function WelcomeChoice({
  title,
  hint,
  on,
  onChange,
}: {
  title: string;
  hint: string;
  on: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={`chip flex w-full items-center gap-3 rounded-2xl px-3.5 py-2.5 text-left ring-1 ring-inset ${
        on ? "bg-white/10 ring-[#0a84ff]" : "bg-white/6 ring-transparent hover:bg-white/9"
      }`}
    >
      <div className="min-w-0 flex-1">
        <p className="font-medium text-white">{title}</p>
        <p className="text-[11.5px] text-[rgb(235_235_245/0.55)]">{hint}</p>
      </div>
      <span
        className={`grid size-5 shrink-0 place-items-center rounded-full text-[11px] font-bold transition-colors duration-150 ${
          on ? "bg-[#0a84ff] text-white" : "bg-white/15 text-transparent"
        }`}
      >
        ✓
      </span>
    </button>
  );
}

export function WelcomeChoiceSkeleton() {
  return (
    <div
      aria-hidden="true"
      className="flex animate-pulse items-center gap-3 rounded-2xl bg-white/6 px-3.5 py-2.5"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="h-3.5 w-24 rounded-md bg-white/10" />
        <div className="h-3 w-44 max-w-full rounded-md bg-white/[0.07]" />
      </div>
      <div className="size-5 shrink-0 rounded-full bg-white/10" />
    </div>
  );
}