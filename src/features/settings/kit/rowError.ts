import { createContext, useContext } from "react";
import { asError, type Settings, type SettingsPatch } from "../../../api";
import { friendly } from "../../../ui/Toast";
import type { Ctx, Entry } from "../registry";

/** What a row gives the controls inside it: its entry, a quiet save, and its result and error lines. */
export type RowApi = {
  entry: Entry;
  ctx: Ctx;
  /** For the control's aria-describedby: the hint and, while there is one, the error. */
  describedBy?: string;
  /** Saves without a toast. A refusal shows at the row and the value goes back; it settles undefined then. */
  write: (patch: SettingsPatch) => Promise<Settings | undefined>;
  /** Shows why an action failed at the row; a cancelled one says nothing. */
  fail: (e: unknown) => void;
  /** The row's error line (role=alert); null clears it. */
  setError: (message: string | null) => void;
  /** The row's result line (role=status): what an action just did; null clears it. */
  setResult: (message: string | null) => void;
  /** After the control that had the focus was swapped out (Schedule… for Cancel), `selector` in the
   * row's controls takes it, unless the focus has moved on outside the row meanwhile. */
  refocus: (selector?: string) => void;
};

export const RowContext = createContext<RowApi | null>(null);

/** The row this control sits in. Only inside a SettingRow. */
export function useRow(): RowApi {
  const row = useContext(RowContext);
  if (!row) throw new Error("useRow is only for controls inside a SettingRow");
  return row;
}

/** A failure in words; null for a cancelled action. */
export function errorText(e: unknown) {
  const err = asError(e);
  return err.kind === "cancelled" ? null : friendly(err);
}
