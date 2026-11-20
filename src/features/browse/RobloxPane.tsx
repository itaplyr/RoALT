import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, asError, type PaneRect } from "../../api";
import { overlayOpen } from "../../ui/a11y";
import { friendly } from "../../ui/Toast";

/** How long Browse may stay hidden (another view on screen) before its page closes: a WebView2
 * holds a few hundred MB. */
const IDLE_CLOSE_MS = 5 * 60_000;
let idleClose = 0;

/** Whether a dialog or a menu is open: Roblox's page is a native view over RoALT's own, so it
 * hides under them. */
function useOverlayOpen() {
  const [open, setOpen] = useState(overlayOpen);
  useEffect(() => {
    const check = () => setOpen(overlayOpen());
    const mo = new MutationObserver(check);
    // childList too: a dialog can leave the page while still open.
    mo.observe(document.body, { subtree: true, childList: true, attributes: true, attributeFilter: ["open"] });
    // Popovers (menus, island sheets) say so in a toggle event only; it doesn't bubble, so caught going down.
    document.addEventListener("toggle", check, true);
    return () => {
      mo.disconnect();
      document.removeEventListener("toggle", check, true);
    };
  }, []);
  return open;
}

/** The height the toasts take at the bottom while there are any: the page leaves it free, so they
 * never end up under it. */
function useToastInset() {
  const [inset, setInset] = useState(0);
  useEffect(() => {
    const el = document.querySelector<HTMLElement>(".toaster");
    if (!el) return;
    const ro = new ResizeObserver(() => setInset(el.offsetHeight ? el.offsetHeight + 24 : 0));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return inset;
}

/** Where Roblox's page goes: this element's box, less the toasts' room. RoALT keeps it there as the
 * window and the layout change, and hides it under dialogs and menus. `n` loads `path` again. */
export function RobloxPane({ accountId, path, n, hidden, resume, onError }: {
  accountId: number;
  path: string;
  n: number;
  /** One of RoALT's own tabs is up: the page stays as it is, out of sight. */
  hidden: boolean;
  /** The page last shown: where it opens again if it closed while hidden. */
  resume: string;
  onError: (message: string | null) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const overlay = useOverlayOpen() || hidden;
  const inset = useToastInset();
  // The last box sent: what a cleanup uses, once the element is already gone.
  const last = useRef<PaneRect>({ x: 0, y: 0, width: 1, height: 1 });
  const rect = (): PaneRect => {
    const el = box.current;
    if (!el) return last.current;
    const r = el.getBoundingClientRect();
    last.current = { x: r.left, y: r.top, width: r.width, height: Math.max(1, r.height - inset) };
    return last.current;
  };
  const latest = useRef({ rect, overlay });
  latest.current = { rect, overlay };
  // Browse is still on screen (an open that ends after it left must not show the page).
  const mounted = useRef(true);
  // The page closed while hidden: showing it again opens it again.
  const closed = useRef(false);

  const open = (to: string) => {
    clearTimeout(idleClose);
    closed.current = false;
    let live = true;
    api.browseOpen(accountId, to, latest.current.rect()).then(
      () => {
        // Opening shows the page where the box was: put it where the box is now, or under a dialog.
        if (!mounted.current) return void api.browseBounds(latest.current.rect(), false).catch(() => {});
        if (!live) return;
        onError(null);
        void api.browseBounds(latest.current.rect(), !latest.current.overlay).catch(() => {});
      },
      (e) => live && onError(friendly(asError(e))),
    );
    return () => {
      live = false;
    };
  };
  useEffect(() => open(path), [accountId, path, n]); // eslint-disable-line react-hooks/exhaustive-deps

  // On RoALT's own tabs the page closes too if it isn't shown again within a while. Back, it opens at
  // `resume`, not `path`: that is the last page asked for, and the user may have gone on from it.
  useEffect(() => {
    if (!hidden) {
      if (closed.current) open(resume);
      return;
    }
    const id = window.setTimeout(() => {
      closed.current = true;
      void api.browseClose().catch(() => {});
    }, IDLE_CLOSE_MS);
    return () => clearTimeout(id);
  }, [hidden]); // eslint-disable-line react-hooks/exhaustive-deps

  // Follows the box (window resized, rail dragged) and shows the page unless something is over it.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    let frame = 0;
    const send = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => void api.browseBounds(latest.current.rect(), !latest.current.overlay).catch(() => {}));
    };
    send();
    const ro = new ResizeObserver(send);
    ro.observe(el);
    window.addEventListener("resize", send);
    return () => {
      cancelAnimationFrame(frame);
      ro.disconnect();
      window.removeEventListener("resize", send);
    };
  }, [overlay, inset]);

  // Another view: the page hides at once, and closes if Browse isn't back within a while.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      void api.browseBounds(latest.current.rect(), false).catch(() => {});
      clearTimeout(idleClose);
      idleClose = window.setTimeout(() => void api.browseClose().catch(() => {}), IDLE_CLOSE_MS);
    };
  }, []);

  return <div ref={box} className="browse-pane" />;
}
