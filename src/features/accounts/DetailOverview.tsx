import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, asError, type AccountInfo, type AccountPatch, type AccountSummary, type Group, type Instance, type Presence, type ServerVisit } from "../../api";
import { groupColor, qk, useAccountInfo, useBulk, useInstances, useMultiStatus, useUpdateAccount } from "../../data";
import { fmtDate, fmtList, fmtNum, t, type Key } from "../../i18n";
import { Chip, Disclosure, Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Switch } from "../../ui/controls";
import { Morph } from "../../ui/motion";
import { friendly, toast, toastError } from "../../ui/Toast";
import { launchBlocked, launchOne, useLaunchStep } from "../launch/launchQueue";
import { STATES } from "../launch/Running";
import { accountAge, ago, formatDate, PresenceStatus, Status, SWATCHES } from "./shared";

const HEX = /^#[0-9a-f]{6}$/i;
const NO_ANSWER: Key = "accounts.detail.noAnswer";
/** A client's stage by shape too, as elsewhere: on its way a ring, in game a dot, dropped a diamond. */
const STAGE_SHAPE: Record<Instance["state"], "ring" | "dot" | "diamond"> = { starting: "ring", joining: "ring", inGame: "dot", disconnected: "diamond", exited: "ring" };

export const clip = (text: string, done: string) => writeText(text).then(() => toast(done), toastError);
const openLink = (url: string) => openUrl(url).catch(toastError);

/** A block of the detail panel; blocks are parted by a hairline. */
export function Section({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <section className="acc-sec">
      {label && <h3 className="label text-fg-2">{label}</h3>}
      {children}
    </section>
  );
}

/** A setting's words on one side, its control on the other (under them when there is no room). */
export function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-start justify-between gap-x-4 gap-y-2">
      <span className="flex-[1_1_150px] min-w-0 flex flex-col gap-0.5 break-words">
        <span className="font-medium">{label}</span>
        {hint && <span className="text-[13px] text-fg-3">{hint}</span>}
      </span>
      <span className="ms-auto flex-none">{children}</span>
    </div>
  );
}

/** The account at a glance: its sign-in and what to do about it, where it is, where it was, and how
 * it is organised. Roblox's own details fold away. */
export function DetailOverview({ account: a, groups, checking, instance, onValidate, onLaunch, onRemove }: {
  account: AccountSummary;
  groups: Group[];
  checking: boolean;
  /** Its client, while one is on its way or in a game. */
  instance?: Instance;
  onValidate: () => void;
  onLaunch: () => void;
  onRemove: () => void;
}) {
  const info = useAccountInfo(a.id);
  // Only info that belongs to this Roblox user (local ids can be reused after a removal).
  const d = info.data?.userId === a.userId ? info.data : undefined;
  const session = useMutation({
    mutationFn: () => api.accountOpenSession(a.id),
    onSuccess: () => toast(a.status === "valid" ? t("accounts.detail.sessionOpenedValid") : t("accounts.detail.sessionOpenedOther")),
  });
  const infoError = info.isError && !d && (
    <div className="flex flex-col items-start gap-3">
      <p role="alert" className="alert">{friendly(asError(info.error))}</p>
      <Button size="md" icon="refresh" busy={info.isFetching} onClick={() => info.refetch()}>
        {t("common.tryAgain")}
      </Button>
    </div>
  );

  return (
    <>
      <Section>
        <div className="flex items-center justify-between gap-3 min-h-7">
          <span className="flex items-center gap-2 min-w-0">
            {/* The status keeps its word; only the time beside it gives way. */}
            <span className="flex-none">
              <Status status={a.status} checking={checking} swap />
            </span>
            <span className="meta truncate">
              {a.statusCheckedAt ? t("accounts.detail.checked", { when: ago(a.statusCheckedAt) }) : t("accounts.notValidated")}
            </span>
          </span>
          {a.status !== "unknown" && (
            <Button size="sm" variant="ghost" icon="refresh" busy={checking} onClick={onValidate}>
              {t("accounts.menu.validate")}
            </Button>
          )}
        </div>
        {instance ? (
          <div className="flex items-center justify-between gap-3">
            <span className="flex items-center gap-2 min-w-0 text-fg-2">
              <span aria-hidden="true" className="status-mark" data-shape={STAGE_SHAPE[instance.state]} style={{ color: STATES[instance.state][1] }} />
              <span className="truncate">
                {t("play.already")} · {STATES[instance.state][0]}
              </span>
            </span>
            <Button size="md" icon="monitor" onClick={() => api.instanceShow(instance.id, "focus").catch(toastError)}>
              {t("home.show")}
            </Button>
          </div>
        ) : a.status === "valid" ? (
          // Secondary: the header pill holds the screen's one primary.
          <Button icon="play" block onClick={onLaunch}>
            {t("common.playEllipsis")}
          </Button>
        ) : a.status === "unknown" ? (
          <Button icon="refresh" block busy={checking} onClick={onValidate}>
            {t("accounts.menu.validate")}
          </Button>
        ) : (
          <Button icon="signin" block busy={session.isPending} onClick={() => session.mutate()}>
            {t("accounts.detail.signInAgain")}
          </Button>
        )}
      </Section>

      <Section label={t("accounts.detail.presence")}>
        {d ? <PresenceBlock p={d.presence} /> : infoError || <Skeleton width={160} height={18} />}
      </Section>

      <RecentServers account={a} playing={!!instance} />
      <Organise account={a} groups={groups} />

      <Section>
        <Disclosure summary={t("accounts.detail.more")}>
          <RobloxDetails account={a} d={d} loading={!d && !info.isError} failed={!!infoError} fetching={info.isFetching} />
        </Disclosure>
      </Section>

      <Section>
        <Button variant="danger" icon="trash" block onClick={onRemove}>
          {t("accounts.detail.removeEllipsis")}
        </Button>
      </Section>
    </>
  );
}

export function PresenceBlock({ p }: { p: Presence | null }) {
  if (!p) return <span className="text-fg-3">{t(NO_ANSWER)}</span>;
  if (p.kind !== "inGame") return <PresenceStatus presence={p} />;
  const job = p.gameId;
  const place = p.placeId;
  return (
    <div className="flex flex-col gap-2 min-w-0">
      <PresenceStatus presence={p} />
      {place !== null && (
        <span className="mono text-xs text-fg-3 truncate selectable">
          {job ? t("accounts.detail.placeJob", { place, job: `${job.slice(0, 8)}…${job.slice(-4)}` }) : t("accounts.detail.place", { place })}
        </span>
      )}
      {(place !== null || job) && (
        <div className="flex flex-wrap gap-2">
          {place !== null && (
            <Button size="md" iconEnd="external" onClick={() => openLink(`https://www.roblox.com/games/${place}`)}>
              {t("accounts.detail.openGame")}
            </Button>
          )}
          {job && (
            <Button size="md" icon="copy" onClick={() => clip(job, t("accounts.detail.jobIdCopied"))}>
              {t("accounts.detail.copyJobId")}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}

/** The servers this account's clients reached, newest first; a public one can be joined again. */
export function RecentServers({ account: a, playing }: { account: AccountSummary; playing: boolean }) {
  const history = useQuery({ queryKey: qk.serverHistory(a.id), queryFn: () => api.serverHistory(a.id), staleTime: 0, refetchInterval: 15_000 });
  // With multi-instance off a launch would end the client that runs.
  const multi = useMultiStatus().data;
  const batch = useBulk().data;
  const others = (useInstances().data ?? []).some((i) => i.state !== "exited" && i.accountId !== a.id);
  // Launches go one at a time: the one under way says what it is doing.
  const step = useLaunchStep();
  const blocked = playing ? t("play.already") : (step ?? launchBlocked(others, multi, batch));
  const [joining, setJoining] = useState<string | null>(null);
  const reasonId = useId();

  if (history.isPending) {
    return (
      <Section label={t("accounts.history.title")}>
        {[0, 1].map((i) => (
          <span key={i} className="flex items-center justify-between gap-3" aria-hidden="true">
            <Skeleton width={i ? 110 : 150} height={14} />
            <Skeleton width={64} height={28} radius={999} />
          </span>
        ))}
      </Section>
    );
  }
  const list = history.data?.slice(0, 10) ?? [];
  if (!list.length) return null;

  const rejoin = (v: ServerVisit) => {
    setJoining(v.jobId);
    launchOne(a, { kind: "server", placeId: v.placeId, jobId: v.jobId }).finally(() => setJoining(null));
  };
  const stop = blocked !== null || joining !== null;
  return (
    <Section label={t("accounts.history.title")}>
      <ul className="flex flex-col gap-2.5">
        {list.map((v) => (
          <li key={`${v.jobId}-${v.joinedAt}`} className="flex items-center justify-between gap-3">
            <ServerName visit={v} />
            {v.private ? (
              <span className="flex-none text-[12px] text-fg-3">{t("accounts.history.private")}</span>
            ) : (
              <Button
                size="sm"
                busy={joining === v.jobId}
                aria-disabled={stop || undefined}
                aria-describedby={blocked ? reasonId : undefined}
                onClick={() => !stop && rejoin(v)}
              >
                {t("accounts.history.rejoin")}
              </Button>
            )}
          </li>
        ))}
      </ul>
      {blocked && (
        <p id={reasonId} className="meta">
          {blocked}
        </p>
      )}
    </Section>
  );
}

function ServerName({ visit: v }: { visit: ServerVisit }) {
  const game = useQuery({ queryKey: qk.game(v.placeId), queryFn: () => api.gameInfo(v.placeId) }).data;
  return (
    <span className="flex flex-col min-w-0">
      <span className="truncate">{game?.name ?? t("dashboard.place", { id: v.placeId })}</span>
      <span className="mono text-[12px] text-fg-3">{ago(v.joinedAt)}</span>
    </span>
  );
}

/** What the user calls it, its groups and colour, and how it is treated. Text saves on blur or Enter. */
function Organise({ account: a, groups }: { account: AccountSummary; groups: Group[] }) {
  const update = useUpdateAccount();
  const save = (patch: AccountPatch) => update.mutate({ id: a.id, patch });
  const [alias, setAlias] = useState(a.alias);
  const [notes, setNotes] = useState(a.notes);
  const [hex, setHex] = useState(a.color ?? "");
  const groupsId = useId();
  const colourId = useId();

  const hexBad = hex.trim() !== "" && !HEX.test(hex.trim());
  // A bad value stays, with its error, until it is fixed or cleared.
  const commitHex = () => {
    const v = hex.trim();
    if (v === (a.color ?? "") || hexBad) return;
    save({ color: v.toLowerCase() });
  };
  const toggleGroup = (gid: number) =>
    save({ groupIds: a.groupIds.includes(gid) ? a.groupIds.filter((x) => x !== gid) : [...a.groupIds, gid] });

  return (
    <Section>
      <label className="flex flex-col gap-1.5">
        <span className="label">{t("accounts.detail.alias")}</span>
        <input
          className="field"
          value={alias}
          placeholder={a.displayName}
          maxLength={64}
          onChange={(e) => setAlias(e.target.value)}
          onBlur={() => alias.trim() !== a.alias && save({ alias: alias.trim() })}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
            if (e.key === "Escape" && alias !== a.alias) {
              e.stopPropagation();
              setAlias(a.alias);
            }
          }}
        />
      </label>
      <div className="flex flex-col gap-1.5">
        <span className="label" id={groupsId}>{t("accounts.groups.title")}</span>
        {groups.length === 0 ? (
          <span className="meta">{t("accounts.noGroupsMenu")}</span>
        ) : (
          <div className="flex flex-wrap gap-1.5" role="group" aria-labelledby={groupsId}>
            {groups.map((g) => (
              <Chip key={g.id} small pressed={a.groupIds.includes(g.id)} dot={groupColor(g)} onClick={() => toggleGroup(g.id)}>
                {g.name}
              </Chip>
            ))}
          </div>
        )}
      </div>
      <div className="flex flex-col gap-1.5">
        <span className="label" id={colourId}>{t("accounts.detail.colour")}</span>
        <div className="flex flex-wrap items-center gap-2" role="group" aria-labelledby={colourId}>
          {SWATCHES.map((c) => (
            <button
              key={c}
              type="button"
              className="swatch"
              style={{ background: c }}
              aria-label={t("accounts.groups.colourValue", { color: c })}
              aria-pressed={a.color?.toLowerCase() === c.toLowerCase()}
              onClick={() => {
                setHex(c.toLowerCase());
                save({ color: c.toLowerCase() });
              }}
            />
          ))}
          <input
            className="field mono min-h-8 w-[96px] px-2 text-xs"
            aria-label={t("accounts.detail.customColour")}
            placeholder="#rrggbb"
            value={hex}
            maxLength={7}
            spellCheck={false}
            aria-invalid={hexBad || undefined}
            aria-describedby={hexBad ? `${colourId}-error` : undefined}
            onChange={(e) => setHex(e.target.value)}
            onBlur={commitHex}
            onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
          />
        </div>
        {hexBad && (
          <p id={`${colourId}-error`} className="text-[13px] text-attention">
            {t("accounts.detail.colourInvalid")}
          </p>
        )}
      </div>
      <Row label={t("accounts.detail.favourite")}>
        <Switch label={t("accounts.detail.favourite")} checked={a.favourite} onChange={(favourite) => save({ favourite })} />
      </Row>
      <div className="flex flex-col gap-1">
        <Row label={t("accounts.detail.main")}>
          <Switch label={t("accounts.detail.main")} checked={a.main} onChange={(main) => save({ main })} />
        </Row>
        <Disclosure summary={t("settings.moreAbout", { label: t("accounts.detail.main") })}>{t("accounts.detail.mainHint")}</Disclosure>
      </div>
      <label className="flex flex-col gap-1.5">
        <span className="label">{t("accounts.detail.notes")}</span>
        <textarea className="field" rows={3} value={notes} maxLength={2000} onChange={(e) => setNotes(e.target.value)} onBlur={() => notes !== a.notes && save({ notes })} />
      </label>
    </Section>
  );
}

/** What Roblox says about the account, as of the last ask; Refresh asks again. */
function RobloxDetails({ account: a, d, loading, failed, fetching }: { account: AccountSummary; d?: AccountInfo; loading: boolean; failed: boolean; fetching: boolean }) {
  const qc = useQueryClient();
  const refresh = useMutation({
    mutationFn: () => api.accountInfo(a.id, true),
    onSuccess: (fresh) => {
      qc.setQueryData(qk.info(a.id), fresh);
      // account_info just cached the new avatar URL; the list reads it from there.
      qc.invalidateQueries({ queryKey: qk.headshots });
    },
  });
  return (
    <div className="flex flex-col gap-4 pt-2">
      <div className="flex items-center justify-between gap-2">
        <span className="meta">{d ? t("accounts.detail.updated", { when: ago(d.fetchedAt) }) : failed ? t("accounts.detail.notLoaded") : t("common.loading")}</span>
        <Button size="sm" variant="ghost" icon="refresh" busy={refresh.isPending || fetching} onClick={() => !refresh.isPending && refresh.mutate()}>
          {t("accounts.detail.refresh")}
        </Button>
      </div>
      {d && d.unavailable.length > 0 && <p className="meta">{t("accounts.detail.unavailable", { parts: fmtList(d.unavailable.map(partName)) })}</p>}
      {d ? <Info d={d} /> : loading && <InfoSkeleton />}
      <p className="meta">{t("accounts.detail.addedOn", { date: formatDate(a.createdAt) })}</p>
    </div>
  );
}

function Info({ d }: { d: AccountInfo }) {
  const joined = d.created ? new Date(d.created) : null;
  const when = joined && !Number.isNaN(joined.getTime()) ? joined : null;
  const stats: Stat[] = [
    num("accounts.detail.robux", d.robux),
    num("accounts.detail.friends", d.friends),
    num("accounts.detail.followers", d.followers),
    num("accounts.detail.following", d.following),
    ["accounts.detail.joined", when ? fmtDate(when, { month: "short", year: "numeric" }) : null, when ? fmtDate(when, { dateStyle: "long" }) : undefined],
    ["accounts.detail.age", d.created ? accountAge(d.created) : null],
  ];
  return (
    <>
      <div className="flex flex-wrap gap-1.5">
        {badges(d).map((b) => (
          <span key={b.label} className="tag" style={b.tone ? { color: TONE[b.tone] } : undefined} title={b.title}>
            {b.label}
          </span>
        ))}
      </div>
      <dl className="grid grid-cols-3 gap-x-3 gap-y-3">
        {stats.map(([k, v, title]) => (
          <div key={k} className="min-w-0">
            <dt className="text-[12px] text-fg-3 truncate">{t(k)}</dt>
            <dd className="mono text-[14px] text-fg font-medium truncate" title={v === null ? t(NO_ANSWER) : title}>
              {v ?? "–"}
            </dd>
          </div>
        ))}
      </dl>
      {d.description.trim() && <About text={d.description} />}
    </>
  );
}

/** Label, value (null = no answer), tooltip. */
type Stat = [Key, string | null, string?];

/** Big counts shrink to "1.2M" with the exact number as the tooltip. */
function num(k: Key, n: number | null): Stat {
  if (n === null) return [k, null];
  if (n < 100_000) return [k, fmtNum(n)];
  return [k, fmtNum(n, { notation: "compact", maximumFractionDigits: 1 }), fmtNum(n)];
}

const TONE = { on: "var(--fg)", warn: "var(--attention)" } as const;
type Badge = { label: string; tone?: keyof typeof TONE; title?: string };

function badges(d: AccountInfo): Badge[] {
  const out: Badge[] = [];
  if (d.isBanned) out.push({ label: t("accounts.badge.banned"), tone: "warn" });
  if (d.subscription === "plus") out.push({ label: t("accounts.badge.plus"), tone: "on" });
  if (d.subscription === "premium") out.push({ label: t("accounts.badge.premium"), tone: "on" });
  const m = d.twoStepMethods;
  out.push(
    m === null
      ? { label: t("accounts.badge.twoStepUnknown"), title: t(NO_ANSWER) }
      : m.length
        ? { label: t("accounts.badge.twoStepOn"), tone: "on", title: fmtList(m) }
        : { label: t("accounts.badge.twoStepOff") },
  );
  out.push(
    d.emailVerified === null
      ? { label: t("accounts.badge.emailUnknown"), title: t(NO_ANSWER) }
      : d.emailVerified
        ? { label: t("accounts.badge.emailVerified"), tone: "on" }
        : { label: t("accounts.badge.emailNotVerified") },
  );
  if (d.verifiedBadge) out.push({ label: t("accounts.badge.verified"), tone: "on" });
  return out;
}

/** Three lines, with "Show more" only when the text is actually cut. */
function About({ text }: { text: string }) {
  const p = useRef<HTMLParagraphElement>(null);
  const [cut, setCut] = useState(false);
  const [more, setMore] = useState(false);
  useLayoutEffect(() => {
    const el = p.current;
    if (el) setCut(el.scrollHeight > el.clientHeight + 1);
  }, [text]);
  return (
    <div className="flex flex-col gap-1.5">
      <span className="label text-fg-2">{t("accounts.detail.about")}</span>
      <Morph axis="height">
        <p ref={p} className={`whitespace-pre-line break-words text-fg selectable ${more ? "" : "line-clamp-3"}`}>{text}</p>
      </Morph>
      {cut && (
        <button type="button" className="linkbtn self-start text-[12px] text-fg-3" aria-expanded={more} onClick={() => setMore(!more)}>
          {more ? t("accounts.detail.showLess") : t("accounts.detail.showMore")}
        </button>
      )}
    </div>
  );
}

function InfoSkeleton() {
  return (
    <div aria-busy="true" aria-label={t("accounts.detail.loadingInfo")} className="flex flex-col gap-4">
      <span className="flex gap-1.5">
        {[56, 64, 96].map((w) => (
          <Skeleton key={w} width={w} height={22} radius={999} />
        ))}
      </span>
      <span className="grid grid-cols-3 gap-3">
        {[0, 1, 2, 3, 4, 5].map((i) => (
          <Skeleton key={i} width={52} height={30} />
        ))}
      </span>
    </div>
  );
}

/** The UI name of an AccountInfo part Roblox didn't answer for (ids from `account_info`). */
const PARTS: Record<string, Key> = {
  profile: "accounts.part.profile",
  avatar: "accounts.part.avatar",
  robux: "accounts.part.robux",
  friends: "accounts.part.friends",
  presence: "accounts.part.presence",
  email: "accounts.part.email",
  "2-step": "accounts.part.twoStep",
  subscription: "accounts.part.subscription",
};
const partName = (id: string) => (PARTS[id] ? t(PARTS[id]) : id);
