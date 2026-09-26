#!/usr/bin/env node
// Runs the same commands as .github/workflows/ci.yml before you push.
//
// The job that keeps failing is `rust (windows-2025)`: Clippy only ever
// compiles the host cfg, so `#[cfg(windows)]` modules are invisible on Linux
// and their lints only appear after a push. cargo-xwin compiles the workspace
// for x86_64-pc-windows-msvc locally, which closes that gap.
//
// scripts/ci-local.test.mjs fails when a workflow gains a `run:` command that
// no step here mirrors, so this file cannot silently drift from CI.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Workflow `run:` commands that are runner setup, not project verification.
 * They intentionally have no local counterpart.
 */
export const SETUP_ONLY = [
  "python3 --version",
  "git config --global core.autocrlf false",
  "sudo apt-get update && sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils",
];

/**
 * @typedef {Object} Step
 * @property {string} id
 * @property {string} job CI job mirrored by this step
 * @property {string} runner CI runner mirrored by this step
 * @property {string} ciCommand the workflow `run:` line this covers
 * @property {string[]} command argv run locally
 * @property {string} [requires] executable that must be on PATH
 * @property {string} [install] how to install a missing executable
 * @property {string} [note] why the local command differs from CI
 * @property {Record<string, string>} [env] extra environment for this step
 */

/**
 * @param {NodeJS.Platform} platform
 * @returns {{ steps: Step[], gaps: string[] }}
 */
export function planSteps(platform) {
  const hostRunner = platform === "win32" ? "windows-2025" : "ubuntu-24.04";
  const clippy = "cargo clippy --workspace --all-targets -- -D warnings";
  /** @type {Step[]} */
  const steps = [
    {
      id: "install",
      job: "frontend",
      runner: "ubuntu-24.04",
      ciCommand: "pnpm install --frozen-lockfile",
      command: ["pnpm", "install", "--frozen-lockfile"],
    },
    {
      id: "check",
      job: "frontend",
      runner: "ubuntu-24.04",
      ciCommand: "pnpm check",
      command: ["pnpm", "check"],
    },
    {
      id: "browsers",
      job: "e2e",
      runner: "ubuntu-24.04",
      ciCommand: "pnpm exec playwright install --with-deps chromium",
      command: ["pnpm", "exec", "playwright", "install", "chromium"],
      note: "provisioning, not verification; --with-deps is dropped because the system packages it installs need root",
    },
    {
      id: "e2e",
      job: "e2e",
      runner: "ubuntu-24.04",
      ciCommand: "pnpm test:e2e",
      command: ["pnpm", "test:e2e"],
    },
    {
      id: "fmt",
      job: "rust",
      runner: "ubuntu-24.04 + windows-2025",
      ciCommand: "cargo fmt --all --check",
      command: ["cargo", "fmt", "--all", "--check"],
      note: "rustfmt reads every module regardless of target cfg",
    },
    {
      id: "clippy-host",
      job: "rust",
      runner: hostRunner,
      ciCommand: clippy,
      command: [
        "cargo",
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
      ],
    },
    {
      id: "test-host",
      job: "rust",
      runner: hostRunner,
      ciCommand: "cargo test --workspace",
      command: ["cargo", "test", "--workspace"],
    },
    {
      id: "deny",
      job: "security",
      runner: "ubuntu-24.04",
      ciCommand: "cargo deny check",
      command: ["cargo", "deny", "check"],
      requires: "cargo-deny",
      install: "cargo install --locked cargo-deny",
    },
  ];
  const gaps = [];

  if (platform === "win32") {
    gaps.push(
      "rust (ubuntu-22.04, ubuntu-24.04): Linux-only modules and `cargo test --workspace` need a Linux host",
    );
  } else {
    steps.splice(steps.findIndex((step) => step.id === "clippy-host") + 1, 0, {
      id: "clippy-windows",
      job: "rust",
      runner: "windows-2025",
      ciCommand: clippy,
      command: [
        "cargo",
        "xwin",
        "clippy",
        "--workspace",
        "--all-targets",
        "--target",
        "x86_64-pc-windows-msvc",
        "--",
        "-D",
        "warnings",
      ],
      requires: "cargo-xwin",
      install: "cargo install cargo-xwin --version 0.19.2",
      note: "host Clippy never compiles #[cfg(windows)] modules",
    });
    steps.push({
      id: "test-windows",
      job: "rust",
      runner: "windows-2025",
      ciCommand: "cargo test --workspace",
      command: [
        "cargo",
        "xwin",
        "test",
        "--workspace",
        "--target",
        "x86_64-pc-windows-msvc",
        "--",
        "--test-threads=1",
      ],
      requires: "wine",
      install: "sudo apt-get install -y wine64",
      note: "Clippy only type-checks Windows tests; an assertion that is true on Linux and false on Windows fails only when the binary actually runs",
      env: {
        // Without WINEARCH=win64 wine builds a WOW64 prefix and stops on
        // "wine32 is missing"; the target is 64-bit, so skip the 32-bit half.
        WINEARCH: "win64",
        WINEPREFIX: join(root, "target/.wine"),
        WINEDEBUG: "-all",
      },
    });
    gaps.push(
      "rust (windows-2025): tests run under wine, which is not Windows. Registry, scheduled-task, UAC, and named-pipe behaviour still differ from the CI runner.",
    );
  }

  return { steps, gaps };
}

/**
 * Extracts every `run:` command from a workflow, including block scalars.
 *
 * @param {string} workflow
 * @returns {string[]}
 */
export function workflowRunCommands(workflow) {
  const commands = [];
  const lines = workflow.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const inline = /^(\s*)-?\s*run:\s*(\S.*)$/.exec(lines[index]);
    if (inline && !inline[2].startsWith("|") && !inline[2].startsWith(">")) {
      commands.push(inline[2].trim());
      continue;
    }
    const block = /^(\s*)-?\s*run:\s*[|>][-+]?\s*$/.exec(lines[index]);
    if (!block) continue;
    const body = [];
    for (let next = index + 1; next < lines.length; next += 1) {
      const line = lines[next];
      if (line.trim() !== "" && !/^\s/.test(line)) break;
      if (line.trim() !== "" && line.search(/\S/) <= block[1].length) break;
      body.push(line.trim());
      index = next;
    }
    commands.push(body.join("\n").trim());
  }
  return commands;
}

function hasExecutable(name) {
  const probe = process.platform === "win32" ? "where" : "which";
  return spawnSync(probe, [name], { stdio: "ignore" }).status === 0;
}

function parseArgs(argv) {
  const args = { list: false, bail: false, only: [], skip: [] };
  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index];
    if (token === "--list") args.list = true;
    else if (token === "--bail") args.bail = true;
    else if (token === "--only") args.only = split(argv[++index]);
    else if (token === "--skip") args.skip = split(argv[++index]);
    else if (token.startsWith("--only=")) args.only = split(token.slice(7));
    else if (token.startsWith("--skip=")) args.skip = split(token.slice(7));
    else throw new Error(`unknown argument: ${token}`);
  }
  return args;
}

function split(value) {
  return (value ?? "")
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function format(command) {
  return command.join(" ");
}

function duration(ms) {
  return ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${ms}ms`;
}

function main() {
  const args = parseArgs(process.argv.slice(2));
  const { steps, gaps } = planSteps(process.platform);
  const selected = steps.filter(
    (step) =>
      (args.only.length === 0 || args.only.includes(step.id)) &&
      !args.skip.includes(step.id),
  );

  if (args.list) {
    for (const step of steps) {
      process.stdout.write(
        `${step.id.padEnd(16)} ${step.job} (${step.runner})\n` +
          `${" ".repeat(16)} ci:    ${step.ciCommand}\n` +
          `${" ".repeat(16)} local: ${format(step.command)}\n`,
      );
    }
    return;
  }

  const unknown = [...args.only, ...args.skip].filter(
    (id) => !steps.some((step) => step.id === id),
  );
  if (unknown.length > 0) {
    throw new Error(
      `unknown step id: ${unknown.join(", ")} (run with --list to see them)`,
    );
  }

  const results = [];
  for (const step of selected) {
    const label = `${step.job} (${step.runner})`;
    process.stdout.write(`\n[1m▶ ${step.id}[0m  ${label}\n`);
    if (step.note) process.stdout.write(`  ${step.note}\n`);
    process.stdout.write(`  $ ${format(step.command)}\n\n`);

    if (step.requires && !hasExecutable(step.requires)) {
      process.stdout.write(
        `  ${step.requires} is not installed. A skipped check is how this job breaks in CI.\n` +
          `  install it with: ${step.install}\n`,
      );
      results.push({ step, status: "missing", ms: 0 });
      if (args.bail) break;
      continue;
    }

    const started = Date.now();
    const options = {
      cwd: root,
      stdio: /** @type {const} */ ("inherit"),
      env: { ...process.env, ...step.env },
    };
    const child =
      process.platform === "win32"
        ? spawnSync(format(step.command), { ...options, shell: true })
        : spawnSync(step.command[0], step.command.slice(1), options);
    const ms = Date.now() - started;
    const status = child.status === 0 ? "pass" : "fail";
    results.push({ step, status, ms });
    if (status === "fail" && args.bail) break;
  }

  process.stdout.write(`\n${"─".repeat(72)}\n`);
  for (const { step, status, ms } of results) {
    const mark = { pass: "✔", fail: "✖", missing: "!" }[status];
    process.stdout.write(
      `${mark} ${step.id.padEnd(16)} ${status.padEnd(8)} ${duration(ms).padStart(7)}  ${step.job} (${step.runner})\n`,
    );
  }
  for (const gap of gaps) {
    process.stdout.write(`\nNot covered locally — ${gap}\n`);
  }

  const failed = results.filter((result) => result.status !== "pass");
  if (failed.length > 0) {
    process.stdout.write(
      `\n${failed.length} of ${results.length} checks did not pass.\n`,
    );
    process.exitCode = 1;
    return;
  }
  process.stdout.write(
    `\nAll ${results.length} mirrored checks passed. Remaining risk is listed above.\n`,
  );
}

export function readWorkflow(name) {
  return readFileSync(join(root, ".github/workflows", name), "utf8");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
