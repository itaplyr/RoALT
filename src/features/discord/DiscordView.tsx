import "./discord.css";
import { useId, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { api, asError, type DiscordStatus } from "../../api";
import { qk } from "../../data";
import { t, type Key } from "../../i18n";
import { Disclosure, Skeleton } from "../../ui/Bits";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Field";
import { DUR, EASE_CSS, Morph, Swap } from "../../ui/motion";
import { friendly } from "../../ui/Toast";
import { useRow, type RowApi } from "../settings/kit/rowError";
import { Renderers, SettingRow, type RowRenderers } from "../settings/kit/SettingRow";
import { SettingSection } from "../settings/kit/SettingSection";
import { DISCORD_ENTRIES } from "./entries";

/** Discord's rows of one group, in the order entries.ts gives them. */
const ids = (group?: Key) => DISCORD_ENTRIES.filter((e) => ("group" in e.place ? e.place.group : undefined) === group).map((e) => e.id);

type Busy = "save" | "test" | "remove" | null;

/** The row's API for what one component draws in two of its slots (the control and under the row). */
function WithRow({ children }: { children: (row: RowApi) => ReactNode }) {
  return children(useRow());
}

/** Where alerts go. The vault never gives the URL back: the field only takes a new one, and it
 * empties once that is saved (or the view closes). */
function WebhookRow() {
  const qc = useQueryClient();
  const status = useQuery({ queryKey: qk.discord, queryFn: api.discordGet });
  const s = status.data;
  const [busy, setBusy] = useState<Busy>(null);
  const [url, setUrl] = useState("");
  const [bad, setBad] = useState(false);
  const field = useRef<HTMLInputElement>(null);
  const test = useRef<HTMLButtonElement>(null);
  const lineId = useId();
  // Set, but its id unreadable: the vault is locked.
  const locked = !!s?.configured && !s.webhook;
  const line = status.isError ? friendly(asError(status.error)) : !s ? null : s.webhook ? t("discord.setId", { id: s.webhook }) : t(s.configured ? "discord.setLocked" : "discord.notSet");

  /** One action at a time; what came of it shows at the row. Settles true when it worked. */
  const act = (row: RowApi, what: NonNullable<Busy>, run: () => Promise<DiscordStatus | void>, done: Key) => {
    setBusy(what);
    setBad(false);
    row.setError(null);
    row.setResult(null);
    return run()
      .then(
        (st) => {
          if (st) qc.setQueryData(qk.discord, st);
          row.setResult(t(done));
          return true;
        },
        (e) => {
          row.fail(e);
          return false;
        },
      )
      .finally(() => setBusy(null));
  };
  const save = (row: RowApi) => {
    const u = url.trim();
    if (!u || busy) return;
    void act(row, "save", () => api.discordSet(u), "discord.saved").then((ok) => {
      if (!ok) return setBad(true);
      setUrl("");
      field.current?.animate([{ opacity: 0.3 }, { opacity: 1 }], { duration: DUR.base, easing: EASE_CSS.out });
      // The result line says what's next: a test.
      requestAnimationFrame(() => test.current?.focus());
    });
  };
  const remove = (row: RowApi) => void act(row, "remove", () => api.discordSet(null), "discord.removed").then((ok) => ok && requestAnimationFrame(() => field.current?.focus()));
  const why = locked ? lineId : undefined;
  // The running action's button stays enabled, so the keyboard focus stays on it; a press meanwhile does nothing.
  const other = (what: NonNullable<Busy>) => busy !== null && busy !== what;
  const saveLabel: Key = busy === "save" ? "discord.saving" : s?.configured ? "discord.replace" : "common.save";
  const testLabel: Key = busy === "test" ? "discord.sending" : "discord.sendTest";

  return (
    <SettingRow
      id="webhook"
      detail={
        <Morph axis="height">
          <div id={lineId} className="dc-line">
            {line === null ? (
              <>
                <Skeleton width={280} height={12} className="my-1" />
                <span className="sr-only">{t("common.loading")}</span>
              </>
            ) : (
              <Swap k={line}>{status.isError ? <span className="alert">{line}</span> : line}</Swap>
            )}
          </div>
        </Morph>
      }
      below={
        <WithRow>
          {(row) => (
            <form
              className="dc-form"
              onSubmit={(e) => {
                e.preventDefault();
                save(row);
              }}
            >
              <Input
                ref={field}
                type="password"
                mono
                className="dc-url"
                aria-label={t("discord.urlLabel")}
                aria-describedby={[lineId, row.describedBy].filter(Boolean).join(" ")}
                placeholder="https://discord.com/api/webhooks/…"
                autoComplete="off"
                value={url}
                invalid={bad}
                disabled={locked}
                onChange={(e) => {
                  setUrl(e.target.value);
                  setBad(false);
                  row.setError(null);
                }}
                onKeyDown={(e) => {
                  if (e.key !== "Escape" || !url) return;
                  e.stopPropagation();
                  setUrl("");
                  setBad(false);
                  row.setError(null);
                }}
              />
              <Morph>
                <Button type="submit" size="md" variant={s?.configured ? "secondary" : "primary"} busy={busy === "save"} disabled={locked || other("save") || !url.trim()} aria-describedby={why}>
                  <Swap k={saveLabel}>{t(saveLabel)}</Swap>
                </Button>
              </Morph>
            </form>
          )}
        </WithRow>
      }
    >
      {status.isError ? (
        <Button size="md" icon="refresh" busy={status.isFetching} onClick={() => void status.refetch()}>
          {t("common.tryAgain")}
        </Button>
      ) : (
        <WithRow>
          {(row) => (
            <Morph innerClassName="gap-2">
              <Swap k={s?.configured ? "set" : "unset"}>
                {s?.configured && (
                  <>
                    <Morph>
                      <Button ref={test} size="md" busy={busy === "test"} disabled={locked || other("test")} aria-describedby={why} onClick={() => !busy && void act(row, "test", api.discordTest, "discord.testSent")}>
                        <Swap k={testLabel}>{t(testLabel)}</Swap>
                      </Button>
                    </Morph>
                    <Button size="md" variant="danger" busy={busy === "remove"} disabled={locked || other("remove")} aria-describedby={why} onClick={() => !busy && remove(row)}>
                      {t("discord.removeWebhook")}
                    </Button>
                  </>
                )}
              </Swap>
            </Morph>
          )}
        </WithRow>
      )}
    </SettingRow>
  );
}

const RENDERERS: RowRenderers = { webhook: WebhookRow };

/** Discord: the status on your profile, and alerts to a channel through a webhook (kept in the vault). */
export function DiscordView() {
  return (
    <main className="scroll flex-1 min-w-0 page">
      <Renderers value={RENDERERS}>
        <div className="dc-page">
          <header className="dc-head">
            <h1 className="page-title">{t("app.nav.discord")}</h1>
            <Disclosure className="dc-lead" summary={t("discord.lead")}>
              {t("discord.intro")}
            </Disclosure>
          </header>
          <SettingSection ids={ids()} />
          <div className="dc-events">
            <SettingSection label="discord.events" ids={ids("discord.events")} />
          </div>
        </div>
      </Renderers>
    </main>
  );
}
