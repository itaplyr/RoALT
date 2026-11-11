// While a view is entering (its view transition plays), list rises inside it would double up with
// the view's own fade: riseIn skips during this window.
let until = 0;

export function markViewEntering(ms = 250) {
  until = performance.now() + ms;
}

export const isViewEntering = () => performance.now() < until;
