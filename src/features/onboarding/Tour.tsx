import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { requestNavigation, type View } from "../../app/navigation";
import { t, type Key } from "../../i18n";
import { Button } from "../../ui/Button";
import { flash } from "../../ui/flash";
import { prefersReducedMotion } from "../../ui/motion";

/** The island tile each stop points at (`data-section`), the view it opens, and its words. */
const STOPS: { section: string; view: View; title: Key; text: Key }[] = [
  { section: "home", view: "dashboard", title: "onboarding.tour.homeTitle", text: "onboarding.tour.homeText" },
  { section: "play", view: "launch", title: "onboarding.tour.playTitle", text: "onboarding.tour.playText" },
  { section: "accounts", view: "accounts", title: "onboarding.tour.accountsTitle", text: "onboarding.tour.accountsText" },
  { section: "settings", view: "settings", title: "onboarding.tour.settingsTitle", text: "onboarding.tour.settingsText" },
];

/** A short tour of the real screen: each stop opens its view, lights its island tile and explains it
 * in a bubble below. Esc ends it. Mounted only while it runs. */
export function Tour({ onClose }: { onClose: () => void }) {
  const [at, setAt] = useState(0);
  const bubble = useRef<HTMLDivElement>(null);
  const stop = STOPS[at];
  const last = at === STOPS.length - 1;

  // The bubble's anchor: the stop's tile lends it `--tour` while it's the stop.
  useLayoutEffect(() => {
    requestNavigation(stop.view);
    const tile = document.querySelector<HTMLElement>(`.island [data-section="${stop.section}"]`);
    tile?.style.setProperty("anchor-name", "--tour");
    const pill = tile?.querySelector<HTMLElement>(".island-item") ?? tile;
    if (pill) flash(pill, prefersReducedMotion());
    if (!bubble.current?.matches(":popover-open")) bubble.current?.showPopover();
    const focus = () => bubble.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
    focus();
    // Again once the view is in: Play puts the focus in its own field as it arrives.
    const again = setTimeout(focus, 400);
    return () => {
      clearTimeout(again);
      tile?.style.removeProperty("anchor-name");
    };
  }, [stop]);

  useEffect(() => {
    const keys = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [onClose]);

  return (
    // Manual: a click elsewhere (the tile itself) doesn't end the tour; Esc and Done do.
    <div ref={bubble} popover="manual" role="dialog" aria-labelledby="onb-tour-title" className="menu onb-tour">
      <p className="text-[12px] text-fg-3">{t("onboarding.tour.count", { n: at + 1, total: STOPS.length })}</p>
      <h3 id="onb-tour-title" className="onb-title">{t(stop.title)}</h3>
      <p className="text-fg-2 text-[13px]">{t(stop.text)}</p>
      <div className="flex justify-end gap-2 pt-2">
        {at > 0 && <Button size="md" variant="ghost" onClick={() => setAt(at - 1)}>{t("onboarding.back")}</Button>}
        <Button size="md" variant="primary" data-autofocus onClick={() => (last ? onClose() : setAt(at + 1))}>
          {last ? t("common.done") : t("onboarding.next")}
        </Button>
      </div>
    </div>
  );
}
