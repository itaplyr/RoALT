import { fmtNum, t } from "../../i18n";

// Apart from MonitorView, so Home's cards and menus don't pull the whole Monitor view in.
export const NONE = "—";
export const one = (n: number) => fmtNum(n, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
export const mb = (n: number) => (n >= 1024 ? t("monitor.gb", { n: one(n / 1024) }) : t("monitor.mb", { n: fmtNum(Math.round(n)) }));
export const pct = (n: number | null) => (n === null ? NONE : t("settings.percent", { n: n < 10 ? one(n) : fmtNum(Math.round(n)) }));
