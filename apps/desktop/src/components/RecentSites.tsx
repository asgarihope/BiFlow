import { ArrowRight, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { enabledClients } from "../lib/clients";
import { clientColor, outboundKey } from "../lib/outbound";
import { presetById, type PresetId } from "../lib/presets";
import { useAppStore } from "../store/app";

const SHOWN = 6;

/**
 * The last sites the user routed, newest first, with the route editable in
 * place. Answers "did my site get added, and where does it go?".
 */
export function RecentSites() {
  const { t } = useTranslation();
  const rules = useAppStore((state) => state.rules);
  const settings = useAppStore((state) => state.settings);
  const pinRoute = useAppStore((state) => state.pinRoute);
  const removeRule = useAppStore((state) => state.removeRule);
  const actionPending = useAppStore((state) => state.actionPending);
  const setPage = useAppStore((state) => state.setPage);
  const clients = settings?.clients ?? [];
  const enabled = enabledClients(clients);
  const pins = [...(rules?.pins ?? [])]
    .sort((left, right) => right.created_at.localeCompare(left.created_at))
    .slice(0, SHOWN);

  return (
    <section
      aria-labelledby="recent-sites-title"
      className="h-full rounded-2xl border border-ink/10 bg-surface pb-1"
    >
      <div className="flex items-center justify-between gap-3 px-3.5 pt-3">
        <h2 id="recent-sites-title" className="text-sm font-semibold">
          {t("ui.recent.title")}
        </h2>
        {(rules?.pins.length ?? 0) > 0 ? (
          <button
            type="button"
            onClick={() => setPage("rules", "sites")}
            className="inline-flex items-center gap-1 text-xs font-semibold text-brand hover:underline"
          >
            {t("ui.recent.all", { count: rules?.pins.length ?? 0 })}
            <ArrowRight size={13} aria-hidden className="rtl:rotate-180" />
          </button>
        ) : null}
      </div>
      {pins.length === 0 ? (
        <p className="px-3.5 pb-4 pt-2 text-sm text-muted">
          {t("ui.recent.empty")}
        </p>
      ) : (
        <ul className="mt-1.5 divide-y divide-ink/[0.07]">
          {pins.map((pin) => {
            const key = outboundKey(pin.outbound);
            const direct = key === "direct";
            const color = direct ? undefined : clientColor(key, clients);
            const clientEnabled =
              direct || enabled.some((client) => client.id === key);
            return (
              <li
                key={`${pin.target.kind}:${pin.target.value}`}
                className="group flex items-center gap-2.5 px-3.5 py-2"
              >
                <span
                  aria-hidden
                  className={`h-2 w-2 shrink-0 rounded-full ${direct ? "bg-success" : ""}`}
                  style={color ? { backgroundColor: color } : undefined}
                />
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {pin.target.value}
                </span>
                <select
                  aria-label={t("ui.addSite.routeLabel")}
                  value={key}
                  disabled={actionPending || !clientEnabled}
                  onChange={(event) =>
                    void pinRoute(pin.target.value, event.target.value).catch(
                      () => undefined,
                    )
                  }
                  className="rounded-lg border-transparent bg-ink/[0.05] py-0.5 pe-7 ps-2 text-xs font-semibold hover:bg-ink/[0.08]"
                >
                  <option value="direct">{t("direct")}</option>
                  {(clientEnabled ? enabled : clients).map((client) => (
                    <option key={client.id} value={client.id}>
                      {presetById(client.preset as PresetId).title}
                    </option>
                  ))}
                </select>
                <button
                  type="button"
                  disabled={actionPending}
                  onClick={() => void removeRule(pin.target.value)}
                  aria-label={t("ui.recent.remove", { host: pin.target.value })}
                  className="rounded-md p-1 text-muted opacity-60 transition hover:bg-danger/10 hover:text-danger group-hover:opacity-100 disabled:opacity-30"
                >
                  <X size={14} aria-hidden />
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
