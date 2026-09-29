import { useTranslation } from "react-i18next";
import { enabledClients } from "../lib/clients";
import { defaultRouteFromKey, outboundKey } from "../lib/outbound";
import { presetById, type PresetId } from "../lib/presets";
import { useAppStore } from "../store/app";

/**
 * Where unmatched traffic goes. A client that is down while the stack runs
 * is listed but disabled, with its reason.
 */
export function DefaultRouteSelect() {
  const { t } = useTranslation();
  const settings = useAppStore((state) => state.settings);
  const snapshot = useAppStore((state) => state.snapshot);
  const actionPending = useAppStore((state) => state.actionPending);
  const setDefaultRoute = useAppStore((state) => state.setDefaultRoute);
  if (!settings) return null;
  const live = snapshot?.phase === "running" || snapshot?.phase === "degraded";
  return (
    <select
      data-testid="default-route"
      aria-label={t("defaultRouteLabel")}
      value={outboundKey(settings.default_route)}
      disabled={actionPending}
      onChange={(event) =>
        void setDefaultRoute(defaultRouteFromKey(event.target.value))
      }
      className="rounded-lg border-transparent bg-ink/[0.05] py-1 pe-8 ps-2.5 text-sm font-semibold text-ink hover:bg-ink/[0.08] focus:border-brand/40"
    >
      <option value="direct">{t("direct")}</option>
      {enabledClients(settings.clients).map((client) => {
        const reported = snapshot?.clients.find(
          (item) => item.id === client.id,
        );
        const down =
          live &&
          (reported?.status.phase === "stopped" ||
            reported?.status.phase === "error" ||
            reported?.status.phase === "unavailable");
        const title = presetById(client.preset as PresetId).title;
        return (
          <option key={client.id} value={client.id} disabled={down}>
            {down
              ? `${title} — ${reported?.status.message ?? t("disabled")}`
              : title}
          </option>
        );
      })}
    </select>
  );
}
