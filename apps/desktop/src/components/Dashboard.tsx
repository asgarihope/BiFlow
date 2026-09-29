import {
  ChevronDown,
  Download,
  Eye,
  LoaderCircle,
  Pause,
  Play,
  Power,
  PowerOff,
  ShieldAlert,
  ShieldCheck,
  ShieldOff,
  X,
} from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ComponentPhase, StackSnapshot } from "../api/models";
import { failureReason } from "../lib/failureReason";
import { controlsLocked, isOperating } from "../lib/lifecycle";
import { outboundLabel } from "../lib/outbound";
import { presetById, type PresetId } from "../lib/presets";
import { useAppStore } from "../store/app";
import { AddSiteBar } from "./AddSiteBar";
import { BUTTON_ICON_PX } from "./AppButton";
import { ConnectionActionButton } from "./ConnectionActionButton";
import { DefaultRouteSelect } from "./DefaultRouteSelect";
import { LifecycleCancelButton } from "./LifecycleCancelButton";
import { MihomoConfigDialog } from "./MihomoConfigDialog";
import { RecentSites } from "./RecentSites";
import { StatusPill } from "./StatusPill";
import { TrafficFlow } from "./TrafficFlow";

/**
 * Home: connection state and controls, the add-site bar, three facts, and
 * the live diagram. Component detail is one click away, not five cards.
 */
export function Dashboard({ snapshot }: { snapshot: StackSnapshot }) {
  const { t } = useTranslation();
  const [configOpen, setConfigOpen] = useState(false);
  const active = snapshot.phase === "running" || snapshot.phase === "degraded";

  return (
    <section
      aria-labelledby="dashboard-title"
      className="ui-enter flex flex-col gap-3 pb-2"
    >
      <ConnectionHero snapshot={snapshot} />
      <AddSiteBar />
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Fact
          label={t("exitIp")}
          value={snapshot.exit_ip ?? t("ui.facts.notConnected")}
          mono={Boolean(snapshot.exit_ip)}
        />
        <div data-testid="provider-summary">
          <Fact
            label={t("ui.facts.rules")}
            value={`${snapshot.providers.ready} / ${snapshot.providers.total}`}
            hint={
              snapshot.providers.rules_loaded > 0
                ? t("ui.facts.rulesLoaded", {
                    count: snapshot.providers.rules_loaded,
                  })
                : undefined
            }
          />
        </div>
        <HealthSummary
          snapshot={snapshot}
          onViewConfig={() => setConfigOpen(true)}
        />
      </div>
      {active ? (
        <div className="grid gap-3 lg:grid-cols-5">
          <div className="min-w-0 lg:col-span-3">
            <TrafficFlow />
          </div>
          <div className="min-w-0 lg:col-span-2">
            <RecentSites />
          </div>
        </div>
      ) : (
        <RecentSites />
      )}
      <MihomoConfigDialog open={configOpen} onOpenChange={setConfigOpen} />
    </section>
  );
}

function ConnectionHero({ snapshot }: { snapshot: StackSnapshot }) {
  const { t } = useTranslation();
  const {
    actionPending,
    toggleConnection,
    pauseConnection,
    resumeConnection,
    cancel,
    installingId,
    settings,
  } = useAppStore();
  const active = snapshot.phase === "running" || snapshot.phase === "degraded";
  const paused = snapshot.phase === "paused";
  const locked = controlsLocked(snapshot, actionPending);
  const operating = isOperating(snapshot);
  // A component that is down or missing (helper not installed, client in
  // error) needs attention even before the stack reports an error.
  const componentDown = [
    snapshot.helper,
    ...snapshot.clients.map((client) => client.status),
    snapshot.mihomo,
    snapshot.tun,
    snapshot.dns,
  ].some(({ phase }) => phase === "error" || phase === "unavailable");
  const failed =
    snapshot.phase === "error" || Boolean(snapshot.last_error) || componentDown;
  const tone = active
    ? "text-success bg-success/12"
    : paused
      ? "text-amber-500 bg-amber-400/15"
      : failed
        ? "text-danger bg-danger/10"
        : "text-muted bg-ink/[0.06]";
  const title = active
    ? t("ui.hero.connected")
    : paused
      ? t("ui.hero.paused")
      : operating
        ? t("ui.hero.working")
        : failed
          ? t("ui.hero.attention")
          : t("ui.hero.disconnected");
  const liveName = snapshot.live_route?.match_proxy
    ? outboundLabel(snapshot.live_route.match_proxy, settings?.clients ?? [])
    : null;

  return (
    <div className="flex flex-col gap-3 rounded-2xl border border-ink/10 bg-surface p-4">
      <div className="flex flex-col gap-4 md:flex-row md:items-center">
        <div className="flex min-w-0 flex-1 items-center gap-3.5">
          <span
            className={`relative flex h-12 w-12 shrink-0 items-center justify-center rounded-full ${tone} ${
              active ? "status-orb-live" : ""
            }`}
            aria-hidden
          >
            {active ? (
              <ShieldCheck size={24} />
            ) : failed ? (
              <ShieldAlert size={24} />
            ) : operating ? (
              <LoaderCircle size={24} className="animate-spin" />
            ) : (
              <ShieldOff size={24} />
            )}
          </span>
          <div className="min-w-0">
            <h1
              id="dashboard-title"
              className="text-lg font-semibold tracking-tight"
            >
              {title}
            </h1>
            <div className="mt-0.5 flex flex-wrap items-center gap-x-1.5 gap-y-1 text-sm text-muted">
              <span>{t("ui.hero.iranDirect")}</span>
              <span aria-hidden>·</span>
              <span>{t("ui.hero.restVia")}</span>
              <DefaultRouteSelect />
            </div>
            {liveName ? (
              <p className="sr-only" data-testid="live-match">
                {t("mihomoUsing", { name: liveName })}
              </p>
            ) : null}
          </div>
        </div>
        <div className="flex w-full flex-col gap-2 sm:flex-row sm:flex-nowrap md:w-auto">
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
      </div>
      {snapshot.last_error ? (
        <p
          role="alert"
          data-testid="stack-failure"
          className="rounded-xl bg-danger/10 px-3 py-2 text-sm text-danger"
        >
          {failureReason(snapshot.last_error, t)}
        </p>
      ) : null}
    </div>
  );
}

function Fact({
  label,
  value,
  hint,
  mono,
}: {
  label: string;
  value: string;
  hint?: string;
  mono?: boolean;
}) {
  return (
    <div className="h-full rounded-2xl border border-ink/10 bg-surface px-3.5 py-3">
      <p className="text-xs text-muted">{label}</p>
      <p
        title={value}
        className={`mt-1 truncate font-semibold ${mono ? "font-mono text-[0.95rem]" : ""}`}
      >
        {value}
      </p>
      {hint ? (
        <p className="mt-0.5 truncate text-xs text-muted">{hint}</p>
      ) : null}
    </div>
  );
}

type HealthItem = {
  key: string;
  name: string;
  phase: ComponentPhase;
  message: string | null;
  action?: {
    label: string;
    busy: boolean;
    icon: "install" | "view";
    run: () => void;
  };
};

function phaseTone(phase: ComponentPhase): string {
  if (phase === "running") return "bg-success";
  if (phase === "error" || phase === "unavailable") return "bg-danger";
  if (phase === "starting" || phase === "checking" || phase === "degraded") {
    return "bg-amber-400";
  }
  return "bg-slate-400";
}

/** Five dots and one sentence; the per-component rows open on demand. */
function HealthSummary({
  snapshot,
  onViewConfig,
}: {
  snapshot: StackSnapshot;
  onViewConfig: () => void;
}) {
  const { t } = useTranslation();
  const { dependencies, installingId, installDependency, installHelper } =
    useAppStore();
  const installed = (id: string) =>
    dependencies.find((item) => item.id === id)?.installed !== false;
  const items: HealthItem[] = [
    {
      key: "helper",
      name: t("helper"),
      phase: snapshot.helper.phase,
      message: snapshot.helper.message,
      action:
        snapshot.helper.phase === "unavailable" ||
        snapshot.helper.phase === "error"
          ? {
              label: t("install"),
              busy: installingId === "helper",
              icon: "install",
              run: () => void installHelper(),
            }
          : undefined,
    },
    ...snapshot.clients.map((client) => ({
      key: client.id,
      name: presetById(client.preset as PresetId).title,
      phase: client.status.phase,
      message: client.status.message,
      action:
        client.preset === "hiddify" && !installed("hiddify")
          ? {
              label: t("install"),
              busy: installingId === "hiddify",
              icon: "install" as const,
              run: () => void installDependency("hiddify"),
            }
          : undefined,
    })),
    {
      key: "mihomo",
      name: "Mihomo",
      phase: snapshot.mihomo.phase,
      message: snapshot.mihomo.message,
      action: !installed("mihomo")
        ? {
            label: t("install"),
            busy: installingId === "mihomo",
            icon: "install",
            run: () => void installDependency("mihomo"),
          }
        : snapshot.mihomo.phase === "running"
          ? {
              label: t("viewMihomoConfig"),
              busy: false,
              icon: "view",
              run: onViewConfig,
            }
          : undefined,
    },
    {
      key: "tun",
      name: "TUN",
      phase: snapshot.tun.phase,
      message: snapshot.tun.message,
    },
    {
      key: "dns",
      name: "DNS",
      phase: snapshot.dns.phase,
      message: snapshot.dns.message,
    },
  ];
  const problems = items.filter(
    (item) => item.phase === "error" || item.phase === "unavailable",
  ).length;
  const allRunning = items.every((item) => item.phase === "running");
  // A real problem opens the rows once so its reason and fix are in view;
  // after that the user decides.
  const [open, setOpen] = useState(problems > 0);
  const summary =
    problems > 0
      ? t("ui.health.issues", { count: problems })
      : allRunning
        ? t("ui.health.good")
        : t("ui.health.idle");

  return (
    <>
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={`col-span-2 flex h-full w-full items-center gap-3 rounded-2xl border bg-surface px-3.5 py-3 text-start transition-colors hover:border-ink/20 sm:col-span-1 ${
          problems > 0 ? "border-danger/30" : "border-ink/10"
        }`}
      >
        <div className="min-w-0 flex-1">
          <p className="text-xs text-muted">{t("ui.health.title")}</p>
          <div className="mt-1.5 flex items-center gap-2">
            <span
              data-testid="connection-status-strip"
              className="flex items-center gap-1.5"
            >
              {items.map((item) => (
                <span
                  key={item.key}
                  title={item.name}
                  data-status-light={item.phase}
                  className={`h-2.5 w-2.5 rounded-full ${phaseTone(item.phase)}`}
                />
              ))}
            </span>
            <span
              className={`truncate text-sm font-semibold ${
                problems > 0 ? "text-danger" : ""
              }`}
            >
              {summary}
            </span>
          </div>
        </div>
        <ChevronDown
          size={17}
          aria-hidden
          className={`shrink-0 text-muted transition-transform ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>
      {open ? (
        <ul
          data-testid="health-details"
          className="ui-enter col-span-2 divide-y divide-ink/10 rounded-2xl border border-ink/10 bg-surface sm:col-span-3"
        >
          {items.map((item) => (
            <li
              key={item.key}
              className="flex items-center gap-3 px-3.5 py-2.5 text-sm"
            >
              <span
                aria-hidden
                className={`h-2 w-2 shrink-0 rounded-full ${phaseTone(item.phase)}`}
              />
              <span className="w-20 shrink-0 font-medium">{item.name}</span>
              <span className="min-w-0 flex-1 truncate text-xs text-muted">
                {item.message ?? t("statusDetailUnavailable")}
              </span>
              <StatusPill phase={item.phase} />
              {item.action ? (
                <button
                  type="button"
                  disabled={item.action.busy}
                  onClick={item.action.run}
                  className="inline-flex shrink-0 items-center gap-1 rounded-lg border border-ink/15 px-2.5 py-1 text-xs font-semibold hover:border-brand/40 hover:text-brand disabled:opacity-50"
                >
                  {item.action.busy ? (
                    <LoaderCircle
                      size={13}
                      className="animate-spin"
                      aria-hidden
                    />
                  ) : item.action.icon === "install" ? (
                    <Download size={13} aria-hidden />
                  ) : (
                    <Eye size={13} aria-hidden />
                  )}
                  {item.action.busy ? t("installing") : item.action.label}
                </button>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </>
  );
}
