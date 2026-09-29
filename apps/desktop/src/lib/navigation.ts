import { House, Plug, Route, SettingsIcon, Stethoscope } from "lucide-react";
import type { Page } from "../store/app";

/** The five sections, in order, shared by the sidebar and the bottom nav. */
export const NAV_ITEMS = [
  { page: "dashboard", labelKey: "ui.nav.home", icon: House },
  { page: "rules", labelKey: "ui.nav.routing", icon: Route },
  { page: "clients", labelKey: "ui.nav.clients", icon: Plug },
  { page: "diagnostics", labelKey: "ui.nav.troubleshoot", icon: Stethoscope },
  { page: "settings", labelKey: "ui.nav.settings", icon: SettingsIcon },
] as const satisfies readonly {
  page: Page;
  labelKey: string;
  icon: unknown;
}[];

export type NavPage = (typeof NAV_ITEMS)[number]["page"];
