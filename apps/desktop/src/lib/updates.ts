import type { UpdateProgress } from "./types";

export function updateBusy(progress: UpdateProgress | null): boolean {
  return progress !== null && progress.phase !== "failed";
}

export function updatePercent(progress: UpdateProgress): number | null {
  if (!progress.total) return null;
  return Math.min(100, Math.floor((progress.done / progress.total) * 100));
}

export function updateLabel(progress: UpdateProgress): string {
  switch (progress.phase) {
    case "preparing":
      return "Preparing update...";
    case "downloading": {
      const percent = updatePercent(progress);
      return percent === null ? "Downloading update..." : `Downloading update · ${percent}%`;
    }
    case "verifying":
      return "Verifying download...";
    case "opening":
      return "Opening installer...";
    case "failed":
      return "Update failed";
  }
}

export function updateDetail(progress: UpdateProgress): string {
  const mb = (bytes: number) => `${(bytes / 1_048_576).toFixed(1)} MB`;
  switch (progress.phase) {
    case "preparing":
      return "Connecting and preparing the download.";
    case "downloading":
      return progress.total
        ? `${mb(progress.done)} of ${mb(progress.total)}`
        : progress.done > 0
          ? `${mb(progress.done)} downloaded`
          : "Waiting for the download to start.";
    case "verifying":
      return "Checking the installer before opening it.";
    case "opening":
      return "Duskie will close. Follow the installer to finish updating.";
    case "failed":
      return progress.error ?? "Could not update. Try again.";
  }
}
