// The Launch composer's state and how a target reads in the UI. Shell owns the draft, so it
// survives switching views and is dropped with everything else when the vault locks.
import type { Dispatch, SetStateAction } from "react";
import { api, type ErrorPayload, type LaunchTarget, type ParsedInput, type UserRef } from "../../api";
import { t } from "../../i18n";

export type Draft = {
  text: string;
  /** Place ID typed for a bare job ID. */
  place: string;
  /** The parse of `parsed.text`; lags `text` by the debounce. */
  parsed: { text: string; value: ParsedInput } | null;
  /** A username already looked up (`name` as typed), so launching need not ask again. */
  user: { name: string; ref: UserRef } | null;
  /** Filled from this favourite; typing clears it. */
  favId: number | null;
  /** Where the target came from (a favourite's name, "with @friend"); typing clears it. */
  note: string | null;
};

export const EMPTY_DRAFT: Draft = { text: "", place: "", parsed: null, user: null, favId: null, note: null };

const WEB = "https://www.roblox.com";

/** Text the parser reads back as the same target. */
export function targetText(t: LaunchTarget): string {
  switch (t.kind) {
    case "place":
      return t.launchData
        ? `${WEB}/games/start?placeId=${t.placeId}&launchData=${encodeURIComponent(t.launchData)}`
        : `${WEB}/games/${t.placeId}`;
    case "server":
      return `${WEB}/games/start?placeId=${t.placeId}&gameInstanceId=${t.jobId}`;
    case "privateLink":
      return `${WEB}/games/${t.placeId}?privateServerLinkCode=${t.linkCode}`;
    case "privateAccess":
      return `roblox://experiences/start?placeId=${t.placeId}&accessCode=${t.accessCode}`;
    case "shareLink":
      return `${WEB}/share?code=${t.code}&type=${t.linkType}`;
    case "shareLinkV2":
      return `${WEB}/share?code=${t.code}&v=v2`;
    case "universe":
      return `roblox://navigation/game_details?gameId=${t.universeId}`;
    case "followUser":
      return `roblox://experiences/start?userId=${t.userId}`;
  }
}

/** A draft holding a known target (no parse round trip needed). */
export function draftFor(target: LaunchTarget, favId: number | null = null, note: string | null = null): Draft {
  const text = targetText(target);
  return { ...EMPTY_DRAFT, text, parsed: { text, value: { kind: "target", target } }, favId, note };
}

/** A draft that joins this user (already known, so no lookup). */
export function draftForUser(ref: UserRef): Draft {
  const text = `@${ref.username}`;
  return { ...EMPTY_DRAFT, text, parsed: { text, value: { kind: "username", name: ref.username } }, user: { name: ref.username, ref } };
}

/** Digits, or a roblox.com game link. */
export function placeOf(s: string): number | null {
  const m = /^\s*(\d{1,19})\s*$/.exec(s) ?? /roblox\.com\/(?:[a-z]{2}(?:-[a-z0-9]{2,4})?\/)?games\/(\d{1,19})/i.exec(s);
  const n = m ? Number(m[1]) : 0;
  return n > 0 && Number.isSafeInteger(n) ? n : null;
}

export type Tone = "ok" | "info" | "bad";
export type Look = { kind: string; detail: string; tone: Tone };

const job = (j: string) => `${j.slice(0, 8)}…${j.slice(-4)}`;
const ok = (kind: string, detail: string): Look => ({ kind, detail, tone: "ok" });

/** "Game · place 1818"; link and access codes never show. */
export function describeTarget(tg: LaunchTarget): Look {
  switch (tg.kind) {
    case "place":
      return ok(t("launch.target.game"), t(tg.launchData ? "launch.target.placeWithData" : "launch.target.place", { place: tg.placeId }));
    case "server":
      return ok(t("launch.target.server"), t("launch.target.placeJob", { place: tg.placeId, job: job(tg.jobId) }));
    case "privateLink":
      return ok(t("launch.target.privateServer"), t("launch.target.linkCode", { place: tg.placeId }));
    case "privateAccess":
      return ok(t("launch.target.privateServer"), t("launch.target.accessCode", { place: tg.placeId }));
    case "shareLink":
      return ok(t("launch.target.shareLink"), tg.linkType);
    case "shareLinkV2":
      return ok(t("launch.target.shareLink"), t("launch.target.openedOnLaunch"));
    case "universe":
      return ok(t("launch.target.game"), t("launch.target.universe", { universe: tg.universeId }));
    case "followUser":
      return ok(t("launch.target.joinUser"), t("launch.target.user", { user: tg.userId }));
  }
}

export function describe(p: ParsedInput, place: number | null, user: UserRef | null): Look {
  switch (p.kind) {
    case "target":
      return describeTarget(p.target);
    case "jobIdOnly":
      return place === null
        ? { kind: t("launch.target.server"), detail: t("launch.target.jobNeedsPlace", { job: job(p.jobId) }), tone: "info" }
        : ok(t("launch.target.server"), t("launch.target.placeJob", { place, job: job(p.jobId) }));
    case "username":
      return { kind: t("launch.target.joinUser"), detail: `@${p.name}${user ? ` (${user.displayName})` : ""}`, tone: user ? "ok" : "info" };
    case "invalid":
      return { kind: t("launch.target.notRecognised"), detail: p.reason, tone: "bad" };
    case "unsupported":
      return { kind: t("launch.target.notSupported"), detail: p.reason, tone: "bad" };
  }
}

/** The looked-up user for this name, if any. */
export const userFor = (d: Draft, name: string) =>
  d.user && d.user.name.toLowerCase() === name.toLowerCase() ? d.user.ref : null;

/** How a draft reads now. `pending`: the parse hasn't caught up, which is still launchable because
 * `resolveTarget` parses again. */
export function readDraft(d: Draft) {
  const parsed = d.text.trim() ? d.parsed?.value ?? null : null;
  const place = placeOf(d.place);
  const user = parsed?.kind === "username" ? userFor(d, parsed.name) : null;
  const look = parsed && describe(parsed, place, user);
  const usable = !!look && look.tone !== "bad" && !(parsed?.kind === "jobIdOnly" && place === null);
  return { parsed, place, look, usable, pending: !!d.text.trim() && d.parsed?.text !== d.text };
}

const invalid = (message: string): ErrorPayload => ({ kind: "invalid", message });

/** What the draft launches; parses again if the debounce hasn't caught up, looks a username up. */
export async function resolveTarget(d: Draft, setDraft: Dispatch<SetStateAction<Draft>>, onLookup?: (name: string) => void): Promise<LaunchTarget> {
  const p = d.parsed?.text === d.text ? d.parsed.value : await api.targetParse(d.text);
  switch (p.kind) {
    case "target":
      return p.target;
    case "jobIdOnly": {
      const placeId = placeOf(d.place);
      if (placeId === null) throw invalid(t("launch.enterPlaceId"));
      return { kind: "server", placeId, jobId: p.jobId };
    }
    case "username": {
      let ref = userFor(d, p.name);
      if (!ref) {
        onLookup?.(p.name);
        ref = await api.userLookup(p.name);
        const found = { name: p.name, ref };
        setDraft((x) => (x.text === d.text ? { ...x, user: found } : x));
      }
      return { kind: "followUser", userId: ref.userId };
    }
    default:
      throw invalid(p.reason);
  }
}
