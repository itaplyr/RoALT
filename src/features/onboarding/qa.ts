import { signal } from "../../ui/signal";

/** The QA team's build (`build-installer.ps1 -Qa`): the first-run setup replays every time RoALT starts. */
export const QA = import.meta.env.VITE_ROALT_QA === "1";

/** Shown this run already (QA): a lock and unlock don't replay it, a restart does. */
const KEY = "roalt.onboarding";

export function seenThisRun() {
  try {
    return sessionStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function markSeenThisRun() {
  try {
    sessionStorage.setItem(KEY, "1");
  } catch {
    // Without storage it may show again after a lock; harmless.
  }
}

/** Whether the setup opens on its own now (pure): every run in the QA build, else until it was
 * finished or skipped, and only while there are no accounts (someone upgrading never sees it). */
export function opensOnItsOwn(qa: boolean, onboarded: boolean, accounts: number, seen: boolean) {
  return qa ? !seen : !onboarded && accounts === 0;
}

/** Settings › About's "Start again": the shell opens the setup. */
const replay = signal();
export const onReplay = replay.subscribe;
export const replayOnboarding = replay.notify;
