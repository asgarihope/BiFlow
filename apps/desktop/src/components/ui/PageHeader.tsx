import type { ReactNode } from "react";
import { InfoTip } from "./InfoTip";

/** One-line page title; the explanation, if any, sits behind an info tip. */
export function PageHeader({
  id,
  title,
  info,
  actions,
}: {
  id: string;
  title: string;
  info?: string;
  actions?: ReactNode;
}) {
  return (
    <header className="flex shrink-0 flex-wrap items-center justify-between gap-3">
      <div className="flex min-w-0 items-center gap-1.5">
        <h1 id={id} className="truncate text-xl font-semibold tracking-tight">
          {title}
        </h1>
        {info ? <InfoTip text={info} /> : null}
      </div>
      {actions ? (
        <div className="flex flex-wrap items-center gap-2">{actions}</div>
      ) : null}
    </header>
  );
}
