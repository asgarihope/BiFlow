import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { DirectRulesDocument } from "../api/models";
import { useAppStore } from "../store/app";
import { baseSettings } from "../test/fixtures";
import { DirectRules } from "./DirectRules";

vi.mock("../api/desktop", () => ({
  desktop: {
    listRunningApplications: vi.fn().mockResolvedValue({
      supported: true,
      applications: [{ process_name: "kubectl.exe", instances: 1 }],
    }),
    testRoute: vi.fn().mockResolvedValue({
      target: "example.ir",
      outbound: "direct",
      reason: "custom_rule",
      matched_rule: "example.ir",
      reachable: true,
      tested_at: new Date().toISOString(),
    }),
  },
}));

const rules: DirectRulesDocument = {
  revision: 1,
  pins: [
    {
      target: { kind: "domain", value: "example.ir" },
      outbound: { kind: "direct" },
      list_id: null,
      resolved_ips: ["203.0.113.8"],
      created_at: new Date().toISOString(),
      refreshed_at: new Date().toISOString(),
    },
    {
      target: { kind: "domain", value: "pinned.ir" },
      outbound: {
        kind: "client",
        client_id: "11111111-1111-1111-1111-111111111111",
      },
      list_id: null,
      resolved_ips: ["203.0.113.20"],
      created_at: new Date().toISOString(),
      refreshed_at: new Date().toISOString(),
    },
  ],
  lists: [],
  applications: [],
};

describe("DirectRules", () => {
  it("shows cloud domain and IP counts and last sync", () => {
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      actionPending: false,
      cloudRules: {
        domain_count: 62829,
        ip_count: 2899,
        last_synced_at: "2026-08-12T12:00:00.000Z",
        source: "devlifeX/BiFlow",
        snapshot_revision: "767ef8bf5673",
        sets: [],
      },
      pageTabs: { rules: "iran" },
    });
    render(<DirectRules rules={rules} />);
    expect(screen.getByText(/62[,\u00a0\s]?829/)).toBeVisible();
    expect(screen.getByText(/2[,\u00a0\s]?899/)).toBeVisible();
    expect(
      screen.getByRole("button", { name: /update from cloud/i }),
    ).toBeEnabled();
  });

  it("adds a pasted site through the chosen route and confirms it", async () => {
    const pinRoute = vi.fn().mockResolvedValue(undefined);
    const showToast = vi.fn();
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      actionPending: false,
      pageTabs: {},
      pinRoute,
      showToast,
    });
    render(<DirectRules rules={rules} />);
    expect(screen.getByRole("tab", { name: "Sites" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await userEvent.type(
      screen.getByLabelText("Site or IP"),
      "https://www.aparat.com/v/1",
    );
    await userEvent.click(screen.getByRole("radio", { name: "DIRECT" }));
    await userEvent.click(screen.getByRole("button", { name: /^Add$/ }));
    expect(pinRoute).toHaveBeenCalledWith("www.aparat.com", "direct");
    expect(showToast).toHaveBeenCalledWith("www.aparat.com → DIRECT");
    expect(screen.getByLabelText("Site or IP")).toHaveValue("");
  });

  it("keeps the input when adding a site fails", async () => {
    const pinRoute = vi.fn().mockRejectedValue(new Error("rules changed"));
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      actionPending: false,
      pageTabs: {},
      pinRoute,
    });
    render(<DirectRules rules={rules} />);
    await userEvent.type(screen.getByLabelText("Site or IP"), "aparat.com");
    await userEvent.click(screen.getByRole("button", { name: /^Add$/ }));
    expect(pinRoute).toHaveBeenCalled();
    expect(screen.getByLabelText("Site or IP")).toHaveValue("aparat.com");
  });

  it("marks an empty site instead of submitting it", async () => {
    const pinRoute = vi.fn();
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      pageTabs: {},
      pinRoute,
    });
    render(<DirectRules rules={rules} />);
    await userEvent.type(screen.getByLabelText("Site or IP"), "   ");
    await userEvent.click(screen.getByRole("button", { name: /^Add$/ }));
    expect(pinRoute).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Site or IP")).toHaveAttribute(
      "aria-invalid",
      "true",
    );
  });

  it("switches sections with the tab strip and arrow keys", async () => {
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      pageTabs: {},
    });
    render(<DirectRules rules={rules} />);
    const sites = screen.getByRole("tab", { name: "Sites" });
    sites.focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Lists" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    expect(screen.getByTestId("rule-lists")).toBeVisible();
    await userEvent.click(screen.getByRole("tab", { name: "Iran rules" }));
    expect(
      screen.getByRole("button", { name: /update from cloud/i }),
    ).toBeVisible();
    expect(useAppStore.getState().pageTabs.rules).toBe("iran");
  });

  it("routes a detected application and keeps its choice in the saved list", async () => {
    const setApplicationRoute = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({
      settings: baseSettings(),
      rules,
      actionPending: false,
      setApplicationRoute,
      pageTabs: { rules: "apps" },
    });
    render(<DirectRules rules={rules} />);
    const route = await screen.findByRole("combobox", {
      name: "Route for kubectl.exe",
    });
    await userEvent.selectOptions(route, "direct");
    expect(setApplicationRoute).toHaveBeenCalledWith("kubectl.exe", "direct");
  });
});
