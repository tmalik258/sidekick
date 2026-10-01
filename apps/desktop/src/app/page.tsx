import Link from "next/link";

// Only used when previewing in a browser (`pnpm web`); Tauri opens /island
// and /settings directly.
export default function Home() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-3 bg-neutral-900 text-white">
      <h1 className="text-lg font-semibold">Sidekick UI preview</h1>
      <Link className="underline" href="/island/">
        Island
      </Link>
      <Link className="underline" href="/settings/">
        Settings
      </Link>
    </main>
  );
}
