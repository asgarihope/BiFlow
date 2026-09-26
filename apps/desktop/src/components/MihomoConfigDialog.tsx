import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { desktop } from "../api/desktop";
import { highlightYamlLine } from "../lib/yamlHighlight";

export function MihomoConfigDialog({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation();
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return undefined;
    let cancelled = false;
    setText(null);
    setError(null);
    void desktop.runningMihomoConfig().then(
      (value) => {
        if (!cancelled) setText(value);
      },
      (cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      },
    );
    return () => {
      cancelled = true;
    };
  }, [open]);

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/45" />
        <Dialog.Content className="fixed left-1/2 top-1/2 flex max-h-[min(40rem,calc(100vh-2rem))] w-[min(46rem,calc(100vw-2rem))] -translate-x-1/2 -translate-y-1/2 flex-col rounded-2xl bg-surface p-4 shadow-2xl">
          <Dialog.Title className="text-lg font-semibold">
            {t("mihomoConfigTitle")}
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-muted">
            {t("mihomoConfigReadOnly")}
          </Dialog.Description>
          {error ? (
            <p className="mt-3 text-sm text-danger" role="alert">
              {error}
            </p>
          ) : text ? (
            <pre
              data-testid="mihomo-config"
              aria-readonly="true"
              className="mt-3 min-h-0 flex-1 overflow-auto rounded-xl bg-ink/5 p-3 font-mono text-xs leading-5"
            >
              {text.split("\n").map((line, index) => (
                <div
                  key={`${index}-${line.slice(0, 24)}`}
                  className={
                    line.includes("MATCH,") ? "bg-brand/15" : undefined
                  }
                >
                  {highlightYamlLine(line).map((token, tokenIndex) => (
                    <span
                      key={tokenIndex}
                      className={
                        token.kind === "key"
                          ? "text-brand"
                          : token.kind === "comment"
                            ? "text-muted"
                            : token.kind === "string"
                              ? "text-success"
                              : undefined
                      }
                    >
                      {token.text}
                    </span>
                  ))}
                </div>
              ))}
            </pre>
          ) : (
            <p className="mt-3 text-sm text-muted" aria-busy="true">
              {t("mihomoConfigUnavailable")}
            </p>
          )}
          <div className="mt-3 flex justify-end border-t border-ink/10 pt-3">
            <Dialog.Close className="rounded-lg bg-brand px-3 py-1.5 text-xs font-semibold text-white">
              {t("close")}
            </Dialog.Close>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
