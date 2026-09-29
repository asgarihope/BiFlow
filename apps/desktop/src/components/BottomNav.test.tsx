import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { useAppStore } from "../store/app";
import { BottomNav } from "./BottomNav";

describe("BottomNav", () => {
  it("navigates from the compact bar without a hamburger menu", async () => {
    useAppStore.setState({ page: "dashboard" });
    render(<BottomNav />);
    expect(screen.getByTestId("bottom-nav")).toBeVisible();
    expect(
      screen.queryByRole("button", { name: /menu/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.getAllByRole("button").map((button) => button.textContent),
    ).toEqual(["Home", "Routing", "Clients", "Troubleshoot", "Settings"]);
    await userEvent.click(screen.getByRole("button", { name: "Clients" }));
    expect(useAppStore.getState().page).toBe("clients");
  });
});
