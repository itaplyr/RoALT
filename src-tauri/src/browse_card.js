// Browse's profile card: RoALT's numbers on a player's roblox.com profile. Runs in Browse's page
// (top frame only) before Roblox's own scripts. RoALT hands it the data (`browse_card`); the page
// can't reach RoALT. Text only (textContent, never HTML), in a closed shadow root: Roblox's styles
// don't reach it and its scripts can't read it. Shown only on the profile it belongs to.
(() => {
  if (window.top !== window || location.hostname !== "www.roblox.com") return;
  // Taken before Roblox's scripts run: a page that swaps it can't open the card.
  const attach = Element.prototype.attachShadow;
  const CSS = `
:host { all: initial; position: fixed; left: 16px; bottom: 16px; z-index: 2147483000; }
section { box-sizing: border-box; width: 270px; max-height: 60vh; overflow: auto; padding: 12px 14px;
  border-radius: 14px; background: #121212; color: #ededed; border: 1px solid #2c2c2c;
  box-shadow: 0 10px 30px rgba(0,0,0,.35); font: 13px/1.4 system-ui, "Segoe UI", sans-serif; }
header { display: flex; align-items: center; gap: 8px; }
header b { font-size: 11px; letter-spacing: .04em; padding: 2px 6px; border-radius: 6px; background: #2a2a2a; }
header span { flex: 1; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
button { all: unset; cursor: pointer; width: 22px; height: 22px; text-align: center; border-radius: 6px; color: #bbb; }
button:hover { background: #2a2a2a; color: #fff; }
button:focus-visible { outline: 2px solid #8ab4ff; }
dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 10px 0 0; }
dt { color: #9a9a9a; } dd { margin: 0; text-align: end; font-variant-numeric: tabular-nums; }
p { margin: 8px 0 0; color: #9a9a9a; }
h4 { margin: 10px 0 4px; font-size: 12px; color: #9a9a9a; font-weight: 600; }
ol { margin: 0; padding: 0; list-style: none; }
li { display: flex; gap: 8px; padding: 2px 0; } li span:first-child { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
li span:last-child { font-variant-numeric: tabular-nums; }`;
  let host = null;
  let root = null;
  let card = null;
  let open = true;
  const el = (tag, text) => {
    const e = document.createElement(tag);
    if (text != null) e.textContent = String(text);
    return e;
  };
  const here = () => card && new RegExp(`^/(?:[a-z]{2}(?:-[a-z]{2,4})?/)?users/${Number(card.userId)}/profile`, "i").test(location.pathname);
  function render() {
    if (!here()) {
      host?.remove();
      return;
    }
    if (!host) {
      host = document.createElement("div");
      root = attach.call(host, { mode: "closed" });
    }
    if (!host.isConnected) document.documentElement.appendChild(host);
    const box = el("section");
    box.setAttribute("aria-label", card.title);
    const head = el("header");
    const toggle = el("button", open ? "–" : "+");
    toggle.setAttribute("aria-expanded", String(open));
    toggle.setAttribute("aria-label", card.toggle);
    toggle.addEventListener("click", () => {
      open = !open;
      render();
    });
    head.append(el("b", "RoALT"), el("span", card.title), toggle);
    box.append(head);
    if (open) {
      const dl = el("dl");
      for (const [k, v] of card.rows) dl.append(el("dt", k), el("dd", v));
      box.append(dl);
      if (card.note) box.append(el("p", card.note));
      if (card.top.length) {
        const ol = el("ol");
        for (const [name, value] of card.top) {
          const li = el("li");
          li.append(el("span", name), el("span", value));
          ol.append(li);
        }
        box.append(el("h4", card.topTitle), ol);
      }
    }
    const style = el("style", CSS);
    root.replaceChildren(style, box);
  }
  // Not replaceable by the page: what RoALT sends goes to this card only.
  Object.defineProperty(window, "__roaltCard", {
    value: (c) => {
      card = c;
      render();
    },
  });
  // Roblox moves between pages in place: the card leaves with its profile.
  let last = location.href;
  setInterval(() => {
    if (location.href !== last) {
      last = location.href;
      render();
    }
  }, 400);
})();
