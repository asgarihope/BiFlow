#!/usr/bin/env node
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { readAppVersion } from "./sync-version.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

export function artifactLayout(fromRoot = root) {
  const version = readAppVersion(fromRoot);
  return {
    version,
    artifactsRoot: "artifacts",
    linux: {
      dir: "artifacts/linux",
      deb: `BiFlow_${version}_amd64.deb`,
      appimage: `BiFlow_${version}_amd64.AppImage`,
    },
    windows: {
      dir: "artifacts/windows",
      exe: "BiFlow.exe",
      installer: `BiFlow_${version}_x64-setup.exe`,
    },
    macos: {
      dir: "artifacts/macos",
      app: "BiFlow.app",
      dmg: `BiFlow_${version}_aarch64.dmg`,
    },
  };
}

function lookup(plan, key) {
  if (key === "version") return plan.version;
  if (key === "json") return `${JSON.stringify(plan, null, 2)}\n`;
  const [os, field] = key.split(".");
  const value = plan[os]?.[field];
  if (typeof value !== "string") {
    throw new Error(`unknown build-plan key: ${key}`);
  }
  return value;
}

function isDirectRun() {
  return Boolean(
    process.argv[1] &&
      import.meta.url === pathToFileURL(resolve(process.argv[1])).href,
  );
}

if (isDirectRun()) {
  const key = process.argv[2] ?? "json";
  process.stdout.write(lookup(artifactLayout(), key));
  if (key !== "json") process.stdout.write("\n");
}
