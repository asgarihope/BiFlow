import { useTranslation } from "react-i18next";
import { NAV_ITEMS, type NavPage } from "../lib/navigation";
import { useAppStore } from "../store/app";

export function BottomNav({
  onNavigate,
}: {
  onNavigate?: (page: NavPage) => void;
}) {
  const { t } = useTranslation();
  const { page: current, setPage } = useAppStore();

  return (
    <nav
      data-testid="bottom-nav"
      aria-label="Primary navigation"
      className="app-bottom-nav flex shrink-0 border-t border-ink/10 bg-surface/95 px-1 py-1 backdrop-blur md:hidden"
    >
      {NAV_ITEMS.map(({ page, icon: Icon, labelKey }) => {
        const active = current === page;
        return (
          <button
            key={page}
            type="button"
            aria-current={active ? "page" : undefined}
            onClick={() => (onNavigate ? onNavigate(page) : setPage(page))}
            className={`flex min-w-0 flex-1 flex-col items-center gap-0.5 rounded-lg px-1 py-1.5 text-[0.65rem] font-medium transition-colors ${
              active ? "text-brand" : "text-muted"
            }`}
          >
            <span
              className={`flex h-7 w-12 items-center justify-center rounded-full transition-colors ${
                active ? "bg-brand/12" : ""
              }`}
            >
              <Icon size={18} aria-hidden />
            </span>
            <span className="max-w-full truncate">{t(labelKey)}</span>
          </button>
        );
      })}
    </nav>
  );
}
