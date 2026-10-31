import { openUrl } from "@tauri-apps/plugin-opener";
import { fmtNum, t } from "../../i18n";
import { Dialog } from "../../ui/Dialog";
import { Icon } from "../../ui/Icon";
import { toastError } from "../../ui/Toast";

const STORE = "https://www.roblox.com/games/102259086923102/Donation-Experience#!/store";

/** The Donation Experience's gamepasses, [Robux, gamepass id].
 * ponytail: a table, so a pass added later needs a release; fetch them if the amounts change often. */
const PASSES: readonly (readonly [number, number])[] = [
  [5, 2060852594], [10, 2061458099], [20, 2058171272], [30, 2061926065], [40, 2057193496], [50, 2058261394],
  [100, 2062783662], [250, 2061746072], [500, 2062957597], [1000, 2062309757], [1250, 2057727406], [1500, 2061758034],
  [2000, 2061500143], [2500, 2061602170], [5000, 2062207829], [10000, 2062225892], [15000, 2062741712], [20000, 2062441750],
  [25000, 2061578048], [50000, 2063041583], [100000, 2062261671], [150000, 2061962044], [200000, 2061122274], [250000, 2062627710],
  [500000, 2061805974], [1000000, 2061878122], [1500000, 2058435366], [2000000, 2061524076], [5000000, 2062759727], [10000000, 2061878123],
];

/** Picks an amount and opens that gamepass in the browser, where whoever is signed in there buys
 * it. RoALT doesn't buy anything and uses no account of its own for this. */
export function DonateDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const go = (url: string) => {
    onClose();
    openUrl(url).catch(toastError);
  };
  return (
    <Dialog open={open} onClose={onClose} title={t("donate.title")} width={520}>
      <div className="flex flex-col gap-4 px-5 pb-5">
        <p className="text-dim">{t("donate.text")}</p>
        <div className="grid gap-2" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(100px, 1fr))" }}>
          {PASSES.map(([robux, id]) => (
            <button key={id} className="btn mono px-2" aria-label={t("donate.amount", { amount: fmtNum(robux) })} onClick={() => go(`https://www.roblox.com/game-pass/${id}/Donation`)}>
              {fmtNum(robux)}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap items-center justify-between gap-2">
          <span className="text-[13px] text-dim">{t("donate.unit")}</span>
          <button className="btn btn-ghost" onClick={() => go(STORE)}>
            <Icon name="external" size={14} />
            {t("donate.store")}
          </button>
        </div>
      </div>
    </Dialog>
  );
}
