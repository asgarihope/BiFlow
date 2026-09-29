import type { KeyboardEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface TabItem {
  id: string;
  label: string;
  icon?: ReactNode;
}

/** Segmented tab strip. Arrow keys follow reading direction. */
export function TabBar({
  label,
  idPrefix,
  tabs,
  value,
  onChange,
}: {
  label: string;
  idPrefix: string;
  tabs: readonly TabItem[];
  value: string;
  onChange: (id: string) => void;
}) {
  const { i18n } = useTranslation();
  const rtl = i18n.dir() === "rtl";

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const index = tabs.findIndex((tab) => tab.id === value);
    const next = rtl ? "ArrowLeft" : "ArrowRight";
    const previous = rtl ? "ArrowRight" : "ArrowLeft";
    let target = -1;
    if (event.key === next) target = (index + 1) % tabs.length;
    else if (event.key === previous)
      target = (index - 1 + tabs.length) % tabs.length;
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = tabs.length - 1;
    const tab = tabs[target];
    if (!tab) return;
    event.preventDefault();
    onChange(tab.id);
    document.getElementById(`${idPrefix}-tab-${tab.id}`)?.focus();
  }

  return (
    <div
      role="tablist"
      aria-label={label}
      onKeyDown={onKeyDown}
      className="flex max-w-full shrink-0 gap-1 self-start overflow-x-auto rounded-xl bg-ink/[0.05] p-1"
    >
      {tabs.map((tab) => {
        const selected = tab.id === value;
        return (
          <button
            key={tab.id}
            type="button"
            role="tab"
            id={`${idPrefix}-tab-${tab.id}`}
            aria-selected={selected}
            aria-controls={`${idPrefix}-panel-${tab.id}`}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(tab.id)}
            className={`inline-flex items-center gap-1.5 whitespace-nowrap rounded-lg px-3 py-1.5 text-sm font-medium transition-colors ${
              selected
                ? "bg-surface text-ink shadow-sm"
                : "text-muted hover:text-ink"
            }`}
          >
            {tab.icon ? (
              <span aria-hidden className="[&>svg]:h-4 [&>svg]:w-4">
                {tab.icon}
              </span>
            ) : null}
            {tab.label}
          </button>
        );
      })}
    </div>
  );
}

export function TabPanel({
  idPrefix,
  value,
  className,
  children,
}: {
  idPrefix: string;
  value: string;
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      key={value}
      role="tabpanel"
      id={`${idPrefix}-panel-${value}`}
      aria-labelledby={`${idPrefix}-tab-${value}`}
      className={`ui-enter flex flex-col gap-3 ${className ?? ""}`}
    >
      {children}
    </div>
  );
}
