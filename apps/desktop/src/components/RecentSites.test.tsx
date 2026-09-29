import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DirectRulesDocument, PinnedRoute } from "../api/models";
import { MOCK_HIDDIFY_ID } from "../lib/outbound";
import { useAppStore } from "../store/app";
import { baseSettings } from "../test/fixtures";
import { RecentSites } from "./RecentSites";

function pin(value: string, created: string, client = false): PinnedRoute {
  return {
    target: { kind: "domain", value },
    outbound: client
      ? { kind: "client", client_id: MOCK_HIDDIFY_ID }
      : { kind: "direct" },
    list_id: null,
    resolved_ips: [],
    created_at: created,
    refreshed_at: null,
  };
}

const rules: DirectRulesDocument = {
  revision: 3,
  pins: [
    pin("old.ir", "2026-09-01T00:00:00Z"),
    pin("new.com", "2026-09-03T00:00:00Z", true),
    pin("mid.ir", "2026-09-02T00:00:00Z"),
  ],
  lists: [],
  applications: [],
};

describe("RecentSites", () => {
  beforeEach(() => {
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      actionPending: false,
      page: "dashboard",
      pageTabs: {},
    });
  });

  it("lists the newest sites first with their route", () => {
    render(<RecentSites />);
    const rows = screen.getAllByRole("listitem");
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining("new.com"),
      expect.stringContaining("mid.ir"),
      expect.stringContaining("old.ir"),
    ]);
    expect(within(rows[0]!).getByRole("combobox")).toHaveValue(MOCK_HIDDIFY_ID);
  });

  it("moves or removes a site in place and opens the full list", async () => {
    const pinRoute = vi.fn().mockResolvedValue(undefined);
    const removeRule = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ pinRoute, removeRule });
    render(<RecentSites />);
    const first = screen.getAllByRole("listitem")[0]!;
    await userEvent.selectOptions(
      within(first).getByRole("combobox"),
      "direct",
    );
    expect(pinRoute).toHaveBeenCalledWith("new.com", "direct");
    await userEvent.click(
      screen.getByRole("button", { name: "Remove old.ir" }),
    );
    expect(removeRule).toHaveBeenCalledWith("old.ir");
    await userEvent.click(screen.getByRole("button", { name: /All 3/ }));
    expect(useAppStore.getState().page).toBe("rules");
    expect(useAppStore.getState().pageTabs.rules).toBe("sites");
  });

  it("invites the first site when nothing is routed yet", () => {
    useAppStore.setState({ rules: { ...rules, pins: [] } });
    render(<RecentSites />);
    expect(
      screen.getByText("Sites you add show up here with their route."),
    ).toBeVisible();
  });
});
