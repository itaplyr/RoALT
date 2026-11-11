import { Button } from "../../../ui/Button";
import { Morph, Swap } from "../../../ui/motion";

/** A row's one-off action: while it runs, its label cross-fades to what it is doing and the width
 * follows. It stays enabled (a disabled button would drop the keyboard focus); a click meanwhile does
 * nothing. */
export function ActionButton({ busy, label, busyLabel, onRun, describedBy }: {
  busy: boolean;
  label: string;
  busyLabel: string;
  onRun: () => void;
  describedBy?: string;
}) {
  return (
    <Morph>
      <Button size="md" busy={busy} aria-describedby={describedBy} onClick={() => !busy && onRun()}>
        <Swap k={busy ? "busy" : "idle"}>{busy ? busyLabel : label}</Swap>
      </Button>
    </Morph>
  );
}
