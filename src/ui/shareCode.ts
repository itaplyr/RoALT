// Share codes: a prefix naming what it is, then JSON as base64url. Pure (scripts/test-look.mjs).

/** `json` as a share code starting `prefix` ("RoALT-PERF:"). */
export function toCode(prefix: string, json: string) {
  const bytes = new TextEncoder().encode(json);
  return prefix + btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The JSON in a share code starting `prefix`; null when it isn't one. */
export function fromCode(prefix: string, code: string): string | null {
  const body = code.trim();
  if (!body.startsWith(prefix)) return null;
  try {
    const b64 = body.slice(prefix.length).replace(/\s+/g, "").replace(/-/g, "+").replace(/_/g, "/");
    return new TextDecoder("utf-8", { fatal: true }).decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
  } catch {
    return null;
  }
}
