import { CheckCircle2, CircleAlert, Info } from "lucide-react";
import { useAppStore } from "../../store/app";

/**
 * Short confirmation after an action ("digikala.com → DIRECT"). Uses
 * aria-live instead of role="status" so it never makes status lookups in the
 * page ambiguous.
 */
export function ToastViewport() {
  const toast = useAppStore((state) => state.toast);
  const dismiss = useAppStore((state) => state.dismissToast);
  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed inset-x-0 bottom-16 z-50 flex justify-center px-4"
    >
      {toast ? (
        <button
          key={toast.id}
          type="button"
          data-testid="toast"
          onClick={dismiss}
          className={`ui-toast pointer-events-auto flex max-w-md items-center gap-2 rounded-xl px-4 py-2.5 text-sm font-medium shadow-card ${
            toast.tone === "danger"
              ? "bg-danger text-white"
              : "bg-ink text-canvas"
          }`}
        >
          {toast.tone === "success" ? (
            <CheckCircle2 size={17} aria-hidden className="text-success" />
          ) : toast.tone === "danger" ? (
            <CircleAlert size={17} aria-hidden />
          ) : (
            <Info size={17} aria-hidden />
          )}
          <span className="min-w-0 truncate">{toast.text}</span>
        </button>
      ) : null}
    </div>
  );
}
