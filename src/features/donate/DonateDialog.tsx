import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";
import { motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { fmtNum, t } from "../../i18n";
import { rovingKeys } from "../../ui/a11y";
import { Button } from "../../ui/Button";
import { Dialog } from "../../ui/Dialog";
import { DUR, RISE, SPRING, staggerDelay } from "../../ui/motion";
import { toastError } from "../../ui/Toast";
import { useOpenCount } from "../vault/useOpenCount";
import "./donate.css";

const STORE = "https://www.roblox.com/games/102259086923102/Donation-Experience#!/store";

/** The Donation Experience's gamepasses, [Robux, gamepass id], in rows of the same magnitude.
 * ponytail: a table, so a pass added later needs a release; fetch them if the amounts change often. */
const ROWS: readonly (readonly (readonly [number, number])[])[] = [
  [[5, 2060852594], [10, 2061458099], [20, 2058171272], [30, 2061926065], [40, 2057193496], [50, 2058261394]],
  [[100, 2062783662], [250, 2061746072], [500, 2062957597], [1000, 2062309757], [1250, 2057727406], [1500, 2061758034]],
  [[2000, 2061500143], [2500, 2061602170], [5000, 2062207829], [10000, 2062225892], [15000, 2062741712], [20000, 2062441750]],
  [[25000, 2061578048], [50000, 2063041583], [100000, 2062261671], [150000, 2061962044], [200000, 2061122274], [250000, 2062627710]],
  [[500000, 2061805974], [1000000, 2061878122], [1500000, 2058435366], [2000000, 2061524076], [5000000, 2062759727], [10000000, 2061878123]],
];
const COLS = 6;

/** Picks an amount and opens that gamepass in the browser, where whoever is signed in there buys
 * it. RoALT doesn't buy anything and uses no account of its own for this. */
export function DonateDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const opened = useOpenCount(open);
  const textId = useId();
  return (
    <Dialog open={open} onClose={onClose} title={t("donate.title")} width={520} describedBy={textId}>
      <Amounts key={opened} textId={textId} onClose={onClose} />
    </Dialog>
  );
}

/** One opening's amounts: they rise in, and the one picked stays pressed while the dialog goes. */
function Amounts({ textId, onClose }: { textId: string; onClose: () => void }) {
  const [picked, setPicked] = useState<number | null>(null);
  // Roving tabindex: the grid is one Tab stop, arrows move inside it.
  const [active, setActive] = useState(ROWS[0][0][1]);
  const closing = useRef(0);
  useEffect(() => () => clearTimeout(closing.current), []);

  const pick = (id: number) => {
    if (picked !== null) return;
    setPicked(id);
    openUrl(`https://www.roblox.com/game-pass/${id}/Donation`).catch(toastError);
    closing.current = window.setTimeout(onClose, DUR.fast);
  };
  // Arrows move through the grid: along a row (rovingKeys), or a row up or down.
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return void rovingKeys(e, "button");
    const all = [...e.currentTarget.querySelectorAll<HTMLElement>("button")];
    const to = all[all.indexOf(document.activeElement as HTMLElement) + (e.key === "ArrowDown" ? COLS : -COLS)];
    if (!to) return;
    e.preventDefault();
    to.focus();
  };

  return (
    <>
      <div className="dlg-body flex flex-col gap-3">
        <p id={textId} className="text-fg-2">{t("donate.text")}</p>
        <div className="donate-grid" role="group" aria-label={t("donate.unit")} onKeyDown={keys}>
          {ROWS.map((row, r) => (
            <div key={r} className="donate-row">
              {row.map(([robux, id], c) => {
                const full = t("donate.amount", { amount: fmtNum(robux) });
                const short = robux >= 10_000 ? fmtNum(robux, { notation: "compact" }) : null;
                return (
                  <motion.button
                    key={id}
                    type="button"
                    className="chip donate-amount"
                    data-picked={picked === id || undefined}
                    tabIndex={active === id ? 0 : -1}
                    onFocus={() => setActive(id)}
                    // The name starts with what shows ("10K"), so voice control can find it.
                    aria-label={short ? `${short} · ${full}` : full}
                    data-tip={short ? full : undefined}
                    initial={{ opacity: 0, y: RISE.sm }}
                    animate={{ opacity: 1, y: 0, scale: picked === id ? 0.95 : 1 }}
                    // A diagonal wave in reading order, capped; the press itself starts at once.
                    transition={{ ...SPRING.smooth, delay: picked === null ? DUR.instant / 1000 + staggerDelay(r + c) : 0 }}
                    onClick={() => pick(id)}
                  >
                    {short ?? fmtNum(robux)}
                  </motion.button>
                );
              })}
            </div>
          ))}
        </div>
      </div>
      <div className="dlg-foot items-center justify-between">
        <span className="text-[13px] text-fg-3">{t("donate.unit")}</span>
        <Button
          variant="ghost"
          size="md"
          iconEnd="external"
          onClick={() => {
            openUrl(STORE).catch(toastError);
            onClose();
          }}
        >
          {t("donate.store")}
        </Button>
      </div>
    </>
  );
}
