import { Info } from "lucide-react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

/**
 * Explanations live here instead of paragraphs under every heading: a small
 * icon that reveals the text on hover or keyboard focus.
 */
export function InfoTip({ text }: { text: string }) {
  const { t } = useTranslation();
  const id = useId();
  return (
    <span className="group relative inline-flex">
      <button
        type="button"
        aria-label={t("ui.moreInfo")}
        aria-describedby={id}
        className="rounded-full p-0.5 text-muted transition-colors hover:text-ink focus-visible:text-ink"
      >
        <Info size={15} aria-hidden />
      </button>
      <span
        id={id}
        role="tooltip"
        className="pointer-events-none invisible absolute start-0 top-full z-30 mt-1.5 w-64 max-w-[70vw] rounded-xl border border-ink/10 bg-surface p-2.5 text-xs font-normal leading-5 text-muted opacity-0 shadow-card transition-opacity group-focus-within:visible group-focus-within:opacity-100 group-hover:visible group-hover:opacity-100"
      >
        {text}
      </span>
    </span>
  );
}
