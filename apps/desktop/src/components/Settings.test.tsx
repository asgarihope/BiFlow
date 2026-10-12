import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AppConfig, BootstrapResult } from "../api/models";
import { useAppStore } from "../store/app";
import { baseSettings } from "../test/fixtures";
import { Settings } from "./Settings";

vi.mock("../api/desktop", () => ({
  desktop: {
    validateSettings: vi.fn().mockResolvedValue([]),
  },
}));

const settings: AppConfig = baseSettings();

describe("Settings", () => {
  beforeEach(() => {
    useAppStore.setState({ actionPending: false, pageTabs: {}, boot: null });
  });

  it("preserves a taskbar preference on platforms without a native taskbar", async () => {
    const saveSettings = vi
      .fn<(draft: AppConfig) => Promise<boolean>>()
      .mockResolvedValue(true);
    useAppStore.setState({ saveSettings });
    render(
      <Settings
        settings={{
          ...settings,
          behavior: {
            ...settings.behavior,
            show_taskbar_speed: true,
            taskbar_speed_unit: "megabytes",
          },
        }}
      />,
    );
    await userEvent.click(screen.getByRole("tab", { name: "Behavior" }));
    expect(
      screen.queryByLabelText("Show network speed on taskbar"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByLabelText("Show network speed in panel"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("tab", { name: "Speed display" }),
    ).not.toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Close window to tray"));
    await userEvent.click(
      screen.getByRole("button", { name: "Save settings" }),
    );
    expect(saveSettings.mock.calls[0]?.[0].behavior.show_taskbar_speed).toBe(
      true,
    );
    expect(saveSettings.mock.calls[0]?.[0].behavior.taskbar_speed_unit).toBe(
      "megabytes",
    );
  });

  it.each([
    ["linux", "Show network speed in panel"],
    ["windows", "Show network speed on taskbar"],
  ])(
    "saves %s speed units from a separate tab without changing network settings",
    async (platform, label) => {
      const saveSettings = vi
        .fn<(draft: AppConfig) => Promise<boolean>>()
        .mockResolvedValue(true);
      useAppStore.setState({
        saveSettings,
        boot: { platform } as BootstrapResult,
      });
      render(<Settings settings={settings} />);
      await userEvent.click(screen.getByRole("tab", { name: "Behavior" }));
      expect(screen.queryByLabelText(label)).not.toBeInTheDocument();
      await userEvent.click(screen.getByRole("tab", { name: "Speed display" }));
      const toggle = screen.getByLabelText(label);
      expect(toggle).not.toBeChecked();
      const unit = screen.getByLabelText("Speed unit");
      expect(unit).toHaveValue("auto_bits");
      await userEvent.selectOptions(unit, "megabytes");
      await userEvent.click(toggle);
      await userEvent.click(
        screen.getByRole("button", { name: "Save settings" }),
      );
      expect(saveSettings.mock.calls[0]?.[0].behavior.show_taskbar_speed).toBe(
        true,
      );
      expect(saveSettings.mock.calls[0]?.[0].behavior.taskbar_speed_unit).toBe(
        "megabytes",
      );
      expect(saveSettings.mock.calls[0]?.[0].mihomo).toEqual(settings.mihomo);
    },
  );

  it("lets the operator pick a DIRECT DNS preset including Mokhaberat", async () => {
    const saveSettings = vi
      .fn<(draft: AppConfig) => Promise<boolean>>()
      .mockResolvedValue(true);
    useAppStore.setState({ saveSettings });
    render(<Settings settings={settings} />);
    await userEvent.click(screen.getByRole("tab", { name: "Network" }));
    const dns = screen.getByLabelText("DIRECT DNS");
    expect(dns).toHaveValue("fake_ip");
    expect(screen.getByRole("option", { name: "Fake-ip" })).toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /Mokhaberat \(5\.200\.200\.200\)/ }),
    ).toBeInTheDocument();
    await userEvent.selectOptions(dns, "custom");
    expect(screen.getByLabelText("Custom resolvers")).toBeVisible();
    await userEvent.selectOptions(dns, "mokhaberat");
    expect(screen.queryByLabelText("Custom resolvers")).not.toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "Save settings" }),
    );
    expect(saveSettings.mock.calls[0]?.[0].mihomo.direct_dns_preset).toBe(
      "mokhaberat",
    );
  });
});
