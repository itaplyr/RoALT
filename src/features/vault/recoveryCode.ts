import { createElement, useEffect, useSyncExternalStore } from "react";
import { signal } from "../../ui/signal";
import { RecoveryCodeDialog } from "./RecoveryCodeDialog";

// The newest recovery code, until the user says it is saved. It lives apart from whatever made it:
// a lock mid-flow unmounts the flow, and the code (the old one is void by then) must still be shown
// after the unlock, and never while locked.
let code: string | null = null;
let claims = 0;
const { subscribe, notify: changed } = signal();

/** Keeps a code the backend just returned. Call it straight away, before anything can unmount. */
export function publishRecoveryCode(next: string) {
  code = next;
  changed();
}

/** The user has the code: forget it. */
export function recoveryCodeSaved() {
  code = null;
  changed();
}

/** While `active`, the caller shows the code itself and RecoveryCodeHost stays out of the way.
 * Claim before publishing (a flow claims while it is open), or the host would flash up meanwhile. */
export function useRecoveryCodeClaim(active: boolean) {
  useEffect(() => {
    if (!active) return;
    claims++;
    changed();
    return () => {
      claims--;
      changed();
    };
  }, [active]);
}

/** Shows a code nobody else is showing, once the vault is open. Mount it once, at the root. */
export function RecoveryCodeHost({ unlocked }: { unlocked: boolean }) {
  const pending = useSyncExternalStore(subscribe, () => (claims ? null : code));
  return createElement(RecoveryCodeDialog, { code: unlocked ? pending : null, onClose: recoveryCodeSaved });
}
