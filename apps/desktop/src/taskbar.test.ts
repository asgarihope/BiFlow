import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const root = dirname(fileURLToPath(import.meta.url));

describe("taskbar overlay entry", () => {
  it("does not load the main React store or application shell", () => {
    const source = readFileSync(join(root, "taskbar.ts"), "utf8");
    expect(source).not.toMatch(/store\/app/);
    expect(source).not.toMatch(/from ["'].*main["']/);
    expect(source).not.toMatch(/App\.tsx/);
    expect(source).toMatch(/invoke\("show_main_window"\)/);
    expect(source).toMatch(/listen<TaskbarTrafficView>\("taskbar-traffic"/);
  });
});
