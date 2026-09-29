import { Globe, LoaderCircle, Plus } from "lucide-react";
import { useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { enabledClients } from "../lib/clients";
import { extractHost } from "../lib/host";
import { presetById, type PresetId } from "../lib/presets";
import { useAppStore } from "../store/app";

/**
 * The everyday task in one line: paste a site, pick where it goes, add.
 * Pins the registrable host (a pasted URL is reduced first) and confirms
 * with a toast instead of a paragraph.
 */
export function AddSiteBar() {
  const { t } = useTranslation();
  const settings = useAppStore((state) => state.settings);
  const rules = useAppStore((state) => state.rules);
  const pinRoute = useAppStore((state) => state.pinRoute);
  const showToast = useAppStore((state) => state.showToast);
  const clients = enabledClients(settings?.clients ?? []);
  const options = [
    { id: "direct", label: t("direct") },
    ...clients.map((client) => ({
      id: client.id,
      label: presetById(client.preset as PresetId).title,
    })),
  ];
  const preferred =
    settings?.default_route.kind === "client" &&
    clients.some(
      (client) =>
        settings.default_route.kind === "client" &&
        client.id === settings.default_route.client_id,
    )
      ? settings.default_route.client_id
      : (clients[0]?.id ?? "direct");
  const [choice, setChoice] = useState<string | null>(null);
  const route = options.some((option) => option.id === choice)
    ? (choice as string)
    : preferred;
  const [input, setInput] = useState("");
  const [invalid, setInvalid] = useState(false);
  const [busy, setBusy] = useState(false);

  function submit(event: FormEvent) {
    event.preventDefault();
    const host = extractHost(input);
    if (!host) {
      setInvalid(true);
      return;
    }
    setBusy(true);
    const label = options.find((option) => option.id === route)?.label ?? "";
    void pinRoute(host, route)
      .then(() => {
        setInput("");
        showToast(t("ui.addSite.added", { host, route: label }));
      })
      .catch(() => undefined)
      .finally(() => setBusy(false));
  }

  return (
    <form
      data-testid="add-site"
      onSubmit={submit}
      className="flex flex-col gap-2 rounded-2xl border border-ink/10 bg-surface p-2 shadow-sm sm:flex-row sm:items-center"
    >
      <div className="relative min-w-0 flex-1">
        <Globe
          size={17}
          aria-hidden
          className="pointer-events-none absolute start-3 top-1/2 -translate-y-1/2 text-muted"
        />
        <input
          value={input}
          onChange={(event) => {
            setInput(event.target.value);
            setInvalid(false);
          }}
          aria-label={t("ui.addSite.label")}
          aria-invalid={invalid}
          placeholder={t("ui.addSite.placeholder")}
          className={`w-full rounded-xl bg-canvas ps-9 ${
            invalid ? "border-danger" : "border-ink/10"
          }`}
        />
      </div>
      <RouteToggle
        label={t("ui.addSite.routeLabel")}
        options={options}
        value={route}
        onChange={setChoice}
      />
      <button
        type="submit"
        disabled={busy || !rules}
        className="inline-flex shrink-0 items-center justify-center gap-1.5 rounded-xl bg-brand px-4 py-2.5 text-sm font-semibold text-white transition hover:bg-brand/90 disabled:opacity-50"
      >
        {busy ? (
          <LoaderCircle size={16} className="animate-spin" aria-hidden />
        ) : (
          <Plus size={16} aria-hidden />
        )}
        {t("ui.addSite.add")}
      </button>
    </form>
  );
}

/** Direct / client choice as a segmented control; a select past 3 options. */
export function RouteToggle({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: readonly { id: string; label: string }[];
  value: string;
  onChange: (id: string) => void;
}) {
  if (options.length > 3) {
    return (
      <select
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        className="shrink-0 rounded-xl border-ink/10 bg-canvas text-sm"
      >
        {options.map((option) => (
          <option key={option.id} value={option.id}>
            {option.label}
          </option>
        ))}
      </select>
    );
  }
  return (
    <div
      role="radiogroup"
      aria-label={label}
      className="flex shrink-0 gap-1 rounded-xl bg-ink/[0.05] p-1"
    >
      {options.map((option) => {
        const selected = option.id === value;
        return (
          <button
            key={option.id}
            type="button"
            role="radio"
            aria-checked={selected}
            onClick={() => onChange(option.id)}
            className={`flex-1 whitespace-nowrap rounded-lg px-3 py-1.5 text-sm font-medium transition-colors sm:flex-none ${
              selected
                ? option.id === "direct"
                  ? "bg-surface text-success shadow-sm"
                  : "bg-surface text-brand shadow-sm"
                : "text-muted hover:text-ink"
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
