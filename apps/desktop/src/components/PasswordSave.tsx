"use client";

import { motion } from "motion/react";
import { useEffect, useRef, useState } from "react";
import { api } from "@/lib/bridge";
import type { PasswordEditDraft, PasswordPrompt, PasswordSaved, Suggestion } from "@/lib/types";

const NAMES: Record<string, string> = { chrome: "Chrome", edge: "Edge", brave: "Brave", samsung: "Samsung Internet" };
const names = (ids: string[]) => ids.map((id) => NAMES[id] ?? id).join(", ");

/** Rust owns the deadline. Polling only renders state; it never authorizes a write. */
function usePrompt(id: string, displayed: boolean, onGone?: () => void) {
  const [prompt, setPrompt] = useState<PasswordPrompt | null>(null);
  const [error, setError] = useState<string | null>(null);
  const gone = useRef(onGone);
  gone.current = onGone;
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async (ready: boolean) => {
      try {
        const p = await api.passwordSaveStatus(id, ready);
        if (active) {
          setPrompt(p);
          setError(null);
        }
      } catch (err) {
        if (active) {
          const message = String(err);
          setError(message);
          if (message.includes("gone") || message.includes("expired")) {
            gone.current?.();
            return;
          }
        }
      }
      if (active) timer = setTimeout(() => void refresh(false), 250);
    };
    void refresh(displayed);
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [id, displayed]);
  return { prompt, error };
}

export function PasswordSavePanel({ suggestion }: { suggestion: Suggestion }) {
  const { prompt, error: statusError } = usePrompt(suggestion.id, true);
  const [draft, setDraft] = useState<PasswordEditDraft | null>(null);
  const [source, setSource] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const writing = busy || prompt?.phase === "writing";
  const deadlinePassed = prompt?.phase === "countdown" && prompt.seconds === 0;
  const edit = async () => {
    setBusy(true);
    try {
      setDraft(await api.passwordSaveDraft(suggestion.id));
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };
  const save = async (override: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await api.passwordSaveCommit(suggestion.id, override, draft?.username, draft?.password, source || undefined);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };
  const cancel = () => void api.passwordSaveCancel(suggestion.id).catch((err) => setError(String(err)));
  const seconds = prompt?.seconds ?? 5;
  const title = writing
    ? "Saving local passwords…"
    : draft
      ? "Edit password"
      : prompt?.phase === "retry"
        ? "Save failed; retry manually"
        : prompt?.mirror || prompt?.conflicts.length
          ? `Override expires in ${seconds}`
          : `Auto saving in ${seconds}`;
  return (
    <div className="mt-3 flex flex-col gap-2">
      <p className="font-display text-[15px] font-semibold text-white tabular-nums">{title}</p>
      {prompt?.missing.length ? (
        <p className="text-[12px] text-white/65">
          New entries: {names(prompt.missing)}.{" "}
          {prompt.mirror ? "Skipped unless you choose Override." : "Saved when the timer ends."}
        </p>
      ) : null}
      {prompt?.conflicts.length ? (
        <p className="text-[12px] text-white/65">
          Different passwords in {names(prompt.conflicts)}. Skipped unless you choose Override.
        </p>
      ) : null}
      {prompt?.unavailable.map((r) => (
        <p key={r.browser} className="text-[12px] text-amber-200">
          {NAMES[r.browser] ?? r.browser}: {r.message}
        </p>
      ))}
      {prompt?.mirror && (
        <label className="flex flex-col gap-1 text-[12px] text-white/75">
          Use password from
          <select
            aria-label="Source browser"
            value={source}
            onChange={(e) => setSource(e.target.value)}
            disabled={writing || deadlinePassed}
            className="rounded-lg bg-[#303030] p-2 text-white"
          >
            <option value="">Choose a browser</option>
            {prompt.sources.map((id) => (
              <option key={id} value={id}>
                {NAMES[id] ?? id}
              </option>
            ))}
          </select>
        </label>
      )}
      {draft && <DraftFields draft={draft} onChange={setDraft} disabled={writing} />}
      {(error || statusError) && (
        <p role="alert" className="text-[12px] text-red-300">
          {error || statusError}
        </p>
      )}
      <div className="flex flex-wrap gap-1.5">
        {draft ? (
          <>
            <Chip disabled={writing} onClick={() => void save(false)}>
              Save new entries
            </Chip>
            {prompt?.existing.length ? (
              <Chip primary disabled={writing} onClick={() => void save(true)}>
                Save and override existing passwords
              </Chip>
            ) : null}
          </>
        ) : (
          <>
            {!prompt?.mirror && (
              <Chip disabled={writing || deadlinePassed || !prompt} onClick={() => void edit()}>
                Edit
              </Chip>
            )}
            {(prompt?.mirror || prompt?.conflicts.length) && (
              <Chip
                primary
                disabled={writing || deadlinePassed || !prompt || (prompt.mirror && !source)}
                onClick={() => void save(true)}
              >
                Override existing passwords
              </Chip>
            )}
            {prompt?.phase === "retry" && (
              <Chip disabled={writing} onClick={() => void save(false)}>
                Retry new entries
              </Chip>
            )}
          </>
        )}
        <Chip onClick={cancel}>{prompt?.mirror ? "Cancel mirroring" : "Cancel"}</Chip>
      </div>
    </div>
  );
}

export function PasswordSavedChip({ saved, onGone }: { saved: PasswordSaved; onGone: () => void }) {
  const { prompt, error: statusError } = usePrompt(saved.id, false, onGone);
  const [draft, setDraft] = useState<PasswordEditDraft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const edit = async () => {
    try {
      setDraft(await api.passwordSaveDraft(saved.id));
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };
  const save = async (override: boolean) => {
    if (!draft) return;
    setBusy(true);
    try {
      await api.passwordSaveCommit(saved.id, override, draft.username, draft.password);
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };
  const done = () =>
    void api
      .passwordSaveCancel(saved.id)
      .then(onGone)
      .catch((err) => setError(String(err)));
  return (
    <div className="mt-2 flex flex-col gap-1.5">
      <p className="text-[12px] text-white/65">{saved.message}</p>
      {!draft ? (
        <div className="flex gap-2">
          <span className="text-[13px] text-white/80">Saved {saved.domain}</span>
          <Chip onClick={() => void edit()}>Edit</Chip>
          <Chip onClick={done}>Done</Chip>
        </div>
      ) : (
        <>
          <DraftFields draft={draft} onChange={setDraft} disabled={busy} />
          <p className="text-[12px] text-white/65">Existing entries: {names(prompt?.existing ?? [])}</p>
          <div className="flex flex-wrap gap-1.5">
            <Chip primary disabled={busy} onClick={() => void save(true)}>
              Save and override existing passwords
            </Chip>
            <Chip disabled={busy} onClick={done}>
              Done
            </Chip>
          </div>
        </>
      )}
      {(error || statusError) && (
        <p role="alert" className="text-[12px] text-red-300">
          {error || statusError}
        </p>
      )}
    </div>
  );
}

function DraftFields({
  draft,
  onChange,
  disabled,
}: {
  draft: PasswordEditDraft;
  onChange: (draft: PasswordEditDraft) => void;
  disabled: boolean;
}) {
  const style = "rounded-lg bg-white/12 px-2.5 py-1.5 text-[13px] text-white outline-none focus:bg-white/16";
  return (
    <div className="flex flex-col gap-1.5">
      <input
        aria-label="Username"
        type="text"
        value={draft.username}
        disabled={disabled}
        onChange={(e) => onChange({ ...draft, username: e.target.value })}
        className={style}
        autoComplete="off"
      />
      <input
        aria-label="Password"
        type="password"
        value={draft.password}
        disabled={disabled}
        onChange={(e) => onChange({ ...draft, password: e.target.value })}
        className={style}
        autoComplete="off"
      />
    </div>
  );
}

function Chip({
  children,
  onClick,
  primary,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  primary?: boolean;
  disabled?: boolean;
}) {
  return (
    <motion.button
      type="button"
      disabled={disabled}
      onClick={onClick}
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      className={`chip flex min-h-8 items-center rounded-full px-3 py-1.5 text-[13px] font-medium disabled:opacity-50 ${primary ? "bg-white text-black hover:bg-white/90" : "bg-white/12 text-white hover:bg-white/20"}`}
    >
      {children}
    </motion.button>
  );
}
