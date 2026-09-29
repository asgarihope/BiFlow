import {
  Download,
  Pause,
  Play,
  Power,
  PowerOff,
  ShieldCheck,
  ShieldOff,
  X,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import type { StackSnapshot } from "../api/models";
import { failureReason } from "../lib/failureReason";
import { controlsLocked, isOperating } from "../lib/lifecycle";
import { outboundLabel } from "../lib/outbound";
import { useAppStore } from "../store/app";
import { AddSiteBar } from "./AddSiteBar";
import { AppButton, BUTTON_ICON_PX } from "./AppButton";
import { ConnectionActionButton } from "./ConnectionActionButton";
import { LifecycleCancelButton } from "./LifecycleCancelButton";

export function BasicDashboard({ snapshot }: { snapshot: StackSnapshot }) {
  const { t } = useTranslation();
  const {
    actionPending,
    toggleConnection,
    pauseConnection,
    resumeConnection,
    cancel,
    error,
    installDependency,
    installingId,
    settings,
  } = useAppStore();
  const active = snapshot.phase === "running" || snapshot.phase === "degraded";
  const paused = snapshot.phase === "paused";
  const locked = controlsLocked(snapshot, actionPending);
  const operating = isOperating(snapshot);
  const missing = snapshot.last_error?.remediation === "install_dependency";
  const missingId =
    snapshot.last_error?.code === "MIHOMO_NOT_FOUND" ? "mihomo" : "hiddify";
  const showError =
    error ??
    (snapshot.last_error ? failureReason(snapshot.last_error, t) : null);

  const liveName = snapshot.live_route?.match_proxy
    ? outboundLabel(snapshot.live_route.match_proxy, settings?.clients ?? [])
    : null;

  return (
    <section
      aria-labelledby="basic-dashboard-title"
      className="ui-enter flex min-h-full flex-col items-center justify-center gap-6 px-4 py-6 text-center"
    >
      <div className="flex flex-col items-center gap-3">
        <span
          aria-hidden
          className={`relative flex h-16 w-16 items-center justify-center rounded-full ${
            active
              ? "status-orb-live bg-success/12 text-success"
              : paused
                ? "bg-amber-400/15 text-amber-500"
                : "bg-ink/[0.06] text-muted"
          }`}
        >
          {active ? <ShieldCheck size={30} /> : <ShieldOff size={30} />}
        </span>
        <h1 id="basic-dashboard-title" className="text-2xl font-semibold">
          {active
            ? t("ui.hero.connected")
            : paused
              ? t("ui.hero.paused")
              : t("ui.hero.disconnected")}
        </h1>
        <p className="text-sm text-muted">
          {active && liveName
            ? t("ui.basic.routing", { name: liveName })
            : t("ui.basic.hint")}
        </p>
      </div>

      {showError ? (
        <div
          className="w-full max-w-md rounded-2xl border border-danger/20 bg-danger/5 p-4 text-sm text-danger"
          role="alert"
        >
          <p>{showError}</p>
          {missing ? (
            <AppButton
              icon={<Download size={BUTTON_ICON_PX} aria-hidden />}
              className="mt-3 rounded-xl bg-brand px-4 py-2 font-semibold text-white"
              onClick={() => void installDependency(missingId)}
            >
              {t("install")} {missingId === "mihomo" ? "Mihomo" : "Hiddify"}
            </AppButton>
          ) : null}
        </div>
      ) : null}

      <div className="flex w-full max-w-xl flex-wrap items-center justify-center gap-3">
        {operating && snapshot.operation_id ? (
          <LifecycleCancelButton
            icon={<X size={BUTTON_ICON_PX} aria-hidden />}
            onClick={() => void cancel()}
          />
        ) : null}
        {active ? (
          <ConnectionActionButton
            action="pause"
            snapshot={snapshot}
            installingId={installingId}
            actionPending={actionPending}
            disabled={locked}
            onClick={() => void pauseConnection()}
            icon={<Pause size={BUTTON_ICON_PX} aria-hidden />}
            variant="secondary"
          />
        ) : null}
        {paused ? (
          <ConnectionActionButton
            action="resume"
            snapshot={snapshot}
            installingId={installingId}
            actionPending={actionPending}
            disabled={locked}
            onClick={() => void resumeConnection()}
            icon={<Play size={BUTTON_ICON_PX} aria-hidden />}
            variant="primary"
          />
        ) : null}
        <ConnectionActionButton
          action={active || paused ? "disconnect" : "connect"}
          snapshot={snapshot}
          installingId={installingId}
          actionPending={actionPending}
          disabled={locked}
          onClick={() => void toggleConnection()}
          icon={
            active || paused ? (
              <PowerOff size={BUTTON_ICON_PX} aria-hidden />
            ) : (
              <Power size={BUTTON_ICON_PX} aria-hidden />
            )
          }
          variant={paused ? "secondary" : "primary"}
        />
      </div>

      <div className="w-full max-w-xl text-start">
        <AddSiteBar />
      </div>
    </section>
  );
}
