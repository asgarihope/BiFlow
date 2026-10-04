import { zodResolver } from "@hookform/resolvers/zod";
import { Save } from "lucide-react";
import { useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import type { UseFormRegisterReturn } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";
import { desktop } from "../api/desktop";
import type {
  AppConfig,
  DirectDnsPreset,
  TaskbarTrafficView,
  ValidationIssue,
} from "../api/models";
import {
  DIRECT_DNS_PRESETS,
  DIRECT_DNS_PRESET_SERVERS,
  formatDirectDnsServers,
  parseDirectDnsServers,
} from "../lib/directDns";
import { useAppStore } from "../store/app";
import { About } from "./About";
import { PageHeader } from "./ui/PageHeader";
import { TabBar, TabPanel } from "./ui/Tabs";
import { usePageTab } from "./ui/usePageTab";

const SETTINGS_TABS = ["mihomo", "behavior", "about"] as const;

const TASKBAR_UNIT_OPTIONS = [
  { value: "auto", sample: "9.6 MB" },
  { value: "bytes", sample: "9,600,000 B" },
  { value: "kilobytes", sample: "9,600 KB" },
  { value: "megabytes", sample: "9.6 MB" },
  { value: "gigabytes", sample: "0.01 GB" },
] as const;

const formSchema = z
  .object({
    controllerPort: z.coerce.number().int().min(1).max(65535),
    mixedPort: z.coerce.number().int().min(1).max(65535),
    dnsPort: z.coerce.number().int().min(1).max(65535),
    directDnsPreset: z.enum([
      "fake_ip",
      "shecan",
      "electro",
      "radar",
      "mokhaberat",
      "custom",
    ]),
    directDnsServers: z.string(),
    tunName: z
      .string()
      .min(1)
      .max(64)
      .regex(/^[a-zA-Z0-9_-]+$/),
    logLevel: z.enum(["error", "warn", "info", "debug"]),
    refreshMinutes: z.coerce.number().int().min(1),
    upstreamHours: z.coerce.number().int().min(1),
    launchAtLogin: z.boolean(),
    connectAtLaunch: z.boolean(),
    closeToTray: z.boolean(),
    failClosed: z.boolean(),
    taskbarTrafficEnabled: z.boolean(),
    taskbarTrafficUnit: z.enum([
      "auto",
      "bytes",
      "kilobytes",
      "megabytes",
      "gigabytes",
    ]),
    taskbarTrafficBits: z.boolean(),
  })
  .superRefine((values, context) => {
    if (values.directDnsPreset !== "custom") return;
    if (parseDirectDnsServers(values.directDnsServers).length === 0) {
      context.addIssue({
        code: z.ZodIssueCode.custom,
        path: ["directDnsServers"],
        message: "Enter at least one DNS IP address",
      });
    }
  });

type FormValues = z.infer<typeof formSchema>;

function toValues(config: AppConfig): FormValues {
  return {
    controllerPort: config.mihomo.controller_port,
    mixedPort: config.mihomo.mixed_port,
    dnsPort: config.mihomo.dns_port,
    directDnsPreset: config.mihomo.direct_dns_preset,
    directDnsServers: formatDirectDnsServers(config.mihomo.direct_dns_servers),
    tunName: config.mihomo.tun_name,
    logLevel: config.mihomo.log_level,
    refreshMinutes: config.rules.refresh_interval_minutes,
    upstreamHours: config.rules.upstream_refresh_hours,
    launchAtLogin: config.behavior.launch_at_login,
    connectAtLaunch: config.behavior.connect_at_launch,
    closeToTray: config.behavior.close_to_tray,
    failClosed: config.behavior.fail_closed,
    taskbarTrafficEnabled: config.behavior.taskbar_traffic_enabled,
    taskbarTrafficUnit: config.behavior.taskbar_traffic_unit,
    taskbarTrafficBits: config.behavior.taskbar_traffic_bits,
  };
}

function merge(config: AppConfig, values: FormValues): AppConfig {
  return {
    ...config,
    mihomo: {
      ...config.mihomo,
      controller_port: values.controllerPort,
      mixed_port: values.mixedPort,
      dns_port: values.dnsPort,
      tun_name: values.tunName,
      log_level: values.logLevel,
      direct_dns_preset: values.directDnsPreset,
      direct_dns_servers: parseDirectDnsServers(values.directDnsServers),
    },
    rules: {
      refresh_interval_minutes: values.refreshMinutes,
      upstream_refresh_hours: values.upstreamHours,
    },
    behavior: {
      launch_at_login: values.launchAtLogin,
      connect_at_launch: values.connectAtLaunch,
      close_to_tray: values.closeToTray,
      fail_closed: values.failClosed,
      taskbar_traffic_enabled: values.taskbarTrafficEnabled,
      taskbar_traffic_unit: values.taskbarTrafficUnit,
      taskbar_traffic_bits: values.taskbarTrafficBits,
    },
  };
}

export function Settings({ settings }: { settings: AppConfig }) {
  const { t } = useTranslation();
  const { saveSettings, actionPending } = useAppStore();
  const [tab, setTab] = usePageTab("settings", SETTINGS_TABS);
  const [issues, setIssues] = useState<ValidationIssue[]>([]);
  const {
    register,
    handleSubmit,
    reset,
    watch,
    formState: { errors, isDirty },
  } = useForm<FormValues>({
    resolver: zodResolver(formSchema),
    defaultValues: toValues(settings),
  });
  const directDnsPreset = watch("directDnsPreset");
  const taskbarTrafficEnabled = watch("taskbarTrafficEnabled");
  const taskbarTrafficUnit = watch("taskbarTrafficUnit");
  const taskbarTrafficBits = watch("taskbarTrafficBits");

  useEffect(() => reset(toValues(settings)), [reset, settings]);

  const submit = handleSubmit(async (values) => {
    const draft = merge(settings, values);
    const validation = await desktop.validateSettings(draft);
    setIssues(validation);
    if (validation.length === 0) await saveSettings(draft);
  });

  return (
    <section
      aria-labelledby="settings-title"
      className="flex flex-col gap-3 pb-2"
    >
      <PageHeader id="settings-title" title={t("ui.nav.settings")} />
      <TabBar
        label={t("ui.nav.settings")}
        idPrefix="settings"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: "mihomo", label: t("ui.tabs.network") },
          { id: "behavior", label: t("ui.tabs.behavior") },
          { id: "about", label: t("ui.tabs.about") },
        ]}
      />
      {tab === "about" ? (
        <TabPanel idPrefix="settings" value="about">
          <About />
        </TabPanel>
      ) : (
        <form
          onSubmit={(event) => void submit(event)}
          className="ui-enter flex flex-col"
        >
          <div className="min-h-0 flex-1 overflow-y-auto pe-1">
            {tab === "mihomo" ? (
              <Fieldset
                id="settings-panel-mihomo"
                labelledBy="settings-tab-mihomo"
                legend="Mihomo and network"
              >
                <Field
                  label="Controller port"
                  error={errors.controllerPort?.message}
                >
                  <input
                    type="number"
                    {...register("controllerPort")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <Field label="Mixed port" error={errors.mixedPort?.message}>
                  <input
                    type="number"
                    {...register("mixedPort")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <Field label="DNS port" error={errors.dnsPort?.message}>
                  <input
                    type="number"
                    {...register("dnsPort")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <Field
                  className="sm:col-span-2"
                  label={t("settingsDns.label")}
                  hint={t("settingsDns.help")}
                  error={errors.directDnsPreset?.message}
                >
                  <select
                    {...register("directDnsPreset")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  >
                    {DIRECT_DNS_PRESETS.map((preset) => (
                      <option key={preset} value={preset}>
                        {directDnsOptionLabel(
                          preset,
                          t(`settingsDns.${preset}`),
                        )}
                      </option>
                    ))}
                  </select>
                </Field>
                {directDnsPreset === "custom" ? (
                  <Field
                    className="sm:col-span-2"
                    label={t("settingsDns.customLabel")}
                    hint={t("settingsDns.customHelp")}
                    error={errors.directDnsServers?.message}
                  >
                    <input
                      {...register("directDnsServers")}
                      placeholder={t("settingsDns.customPlaceholder")}
                      className="w-full rounded-xl border-ink/15 bg-canvas"
                    />
                  </Field>
                ) : null}
                <Field label="TUN name" error={errors.tunName?.message}>
                  <input
                    {...register("tunName")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <Field label="Log level" error={errors.logLevel?.message}>
                  <select
                    {...register("logLevel")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  >
                    <option value="error">Error</option>
                    <option value="warn">Warning</option>
                    <option value="info">Info</option>
                    <option value="debug">Debug</option>
                  </select>
                </Field>
              </Fieldset>
            ) : null}

            {tab === "behavior" ? (
              <Fieldset
                id="settings-panel-behavior"
                labelledBy="settings-tab-behavior"
                legend="Behavior and refresh"
              >
                <Field
                  label="Custom rule refresh (minutes)"
                  error={errors.refreshMinutes?.message}
                >
                  <input
                    type="number"
                    {...register("refreshMinutes")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <Field
                  label="Upstream refresh (hours)"
                  error={errors.upstreamHours?.message}
                >
                  <input
                    type="number"
                    {...register("upstreamHours")}
                    className="w-full rounded-xl border-ink/15 bg-canvas"
                  />
                </Field>
                <div className="space-y-3 sm:col-span-2">
                  <Check
                    label="Launch at login"
                    registration={register("launchAtLogin")}
                  />
                  <Check
                    label="Connect at launch"
                    registration={register("connectAtLaunch")}
                  />
                  <Check
                    label="Close window to tray"
                    registration={register("closeToTray")}
                  />
                  <Check
                    label="Block traffic when its client is down (fail closed)"
                    registration={register("failClosed")}
                  />
                </div>
                <div className="space-y-3 sm:col-span-2">
                  <Check
                    label="Show live D / U rates inside the BiFlow tray icon"
                    registration={register("taskbarTrafficEnabled")}
                  />
                  <Field
                    label="Tray badge unit"
                    hint={
                      taskbarTrafficEnabled
                        ? "Auto scales B → KB → MB → GB. Pinned units stay at that magnitude."
                        : "Disabled. The BiFlow tray icon stays as its idle D / U form."
                    }
                    error={errors.taskbarTrafficUnit?.message}
                  >
                    <select
                      {...register("taskbarTrafficUnit")}
                      disabled={!taskbarTrafficEnabled}
                      className="w-full rounded-xl border-ink/15 bg-canvas disabled:opacity-50"
                    >
                      {TASKBAR_UNIT_OPTIONS.map((option) => (
                        <option key={option.value} value={option.value}>
                          {option.value} ({option.sample})
                        </option>
                      ))}
                    </select>
                  </Field>
                  <Check
                    label="Display in bits (Kb / Mb / Gb) instead of bytes"
                    registration={register("taskbarTrafficBits")}
                  />
                  <TaskbarPreview
                    enabled={taskbarTrafficEnabled}
                    unit={taskbarTrafficUnit}
                    bits={taskbarTrafficBits}
                  />
                </div>
              </Fieldset>
            ) : null}
          </div>

          {issues.length > 0 ? (
            <ul
              className="mt-3 shrink-0 rounded-xl border border-danger/20 bg-danger/5 p-4 text-sm text-danger"
              role="alert"
            >
              {issues.map((issue) => (
                <li key={`${issue.field}-${issue.code}`}>
                  {issue.field}: {issue.message}
                </li>
              ))}
            </ul>
          ) : null}

          <button
            disabled={actionPending || !isDirty}
            className="mt-3 inline-flex shrink-0 items-center gap-2 rounded-xl bg-brand px-5 py-3 font-semibold text-white disabled:opacity-50"
          >
            <Save size={18} aria-hidden /> Save settings
          </button>
        </form>
      )}
    </section>
  );
}

function Fieldset({
  id,
  labelledBy,
  legend,
  children,
}: {
  id: string;
  labelledBy: string;
  legend: string;
  children: React.ReactNode;
}) {
  return (
    <fieldset
      id={id}
      role="tabpanel"
      aria-labelledby={labelledBy}
      className="grid gap-4 rounded-2xl border border-ink/10 bg-surface p-3.5 sm:grid-cols-2"
    >
      <legend className="sr-only">{legend}</legend>
      {children}
    </fieldset>
  );
}

function Field({
  label,
  error,
  hint,
  className,
  children,
}: {
  label: string;
  error?: string;
  hint?: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={`block text-sm font-medium ${className ?? ""}`}>
      <label className="block">
        <span className="mb-1.5 block">{label}</span>
        {children}
        {error ? (
          <span className="mt-1 block text-xs text-danger">{error}</span>
        ) : null}
      </label>
      {hint ? (
        <p className="mt-1 text-xs font-normal text-muted">{hint}</p>
      ) : null}
    </div>
  );
}

function directDnsOptionLabel(preset: DirectDnsPreset, name: string): string {
  if (preset === "custom" || preset === "fake_ip") return name;
  return `${name} (${DIRECT_DNS_PRESET_SERVERS[preset].join(", ")})`;
}

function Check({
  label,
  registration,
}: {
  label: string;
  registration: UseFormRegisterReturn;
}) {
  return (
    <label className="flex items-center gap-3 text-sm font-medium">
      <input
        type="checkbox"
        {...registration}
        className="rounded border-ink/20 text-brand focus:ring-brand"
      />
      {label}
    </label>
  );
}

/// Mirrors the live tray-icon badge so the operator can sanity-check
/// the chosen unit / bits mode before they save. Reuses the same Rust
/// formatter via `preview_taskbar_traffic` so this preview and the
/// production icon never disagree on the rendered text.
function TaskbarPreview({
  enabled,
  unit,
  bits,
}: {
  enabled: boolean;
  unit: FormValues["taskbarTrafficUnit"];
  bits: boolean;
}) {
  const [view, setView] = useState<TaskbarTrafficView | null>(null);
  useEffect(() => {
    if (!enabled) {
      setView(null);
      return;
    }
    let cancelled = false;
    const sample = 9_600_000;
    desktop
      .previewTaskbarTraffic(sample, sample / 2)
      .then((value) => {
        if (!cancelled) setView(value);
      })
      .catch(() => {
        if (!cancelled) setView(null);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, unit, bits]);
  return (
    <div
      aria-live="polite"
      className="rounded-xl border border-ink/10 bg-canvas/60 p-3 text-sm"
    >
      <p className="mb-1 text-xs uppercase tracking-wide text-muted">Preview</p>
      {enabled ? (
        view ? (
          <p className="font-mono text-sm">
            <span className="block">D: {view.download}</span>
            <span className="block">U: {view.upload}</span>
          </p>
        ) : (
          <p className="text-xs text-muted">Computing…</p>
        )
      ) : (
        <p className="text-xs text-muted">
          Badge is off. Toggle the checkbox above to enable the live D / U
          readout inside the BiFlow tray icon.
        </p>
      )}
    </div>
  );
}
