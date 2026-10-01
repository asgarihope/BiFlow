import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const assets = [
  {
    name: "Linux Mihomo",
    path: "vendor/mihomo/linux-x86_64/mihomo",
    sha256: "9c397be7489538628fae781bc005e4c5b8cd7b0961b8bb2ca815c8150f193577",
    magic: Buffer.from([0x7f, 0x45, 0x4c, 0x46]),
  },
  {
    name: "Windows Mihomo",
    path: "vendor/mihomo/windows-x86_64/mihomo.exe",
    sha256: "4316ff91fecec2fca9acb5612d7400ba228c069ffd325b1f17f46f1d4ef7e0cd",
    magic: Buffer.from("MZ"),
  },
  {
    name: "macOS Mihomo",
    path: "vendor/mihomo/darwin/mihomo",
    sha256: "ec66e3e883bdc3fca06753784e324e08921e13239f8e945587cb1bfbf4c6b936",
    magic: Buffer.from([0xcf, 0xfa, 0xed, 0xfe]),
  },
  {
    name: "Windows Wintun",
    path: "vendor/wintun/windows-x86_64/wintun.dll",
    sha256: "e5da8447dc2c320edc0fc52fa01885c103de8c118481f683643cacc3220dafce",
    magic: Buffer.from("MZ"),
  },
];

for (const asset of assets) {
  const bytes = readFileSync(join(root, asset.path));
  assert.ok(
    bytes.subarray(0, asset.magic.length).equals(asset.magic),
    `${asset.name} has an invalid executable header`,
  );
  const actual = createHash("sha256").update(bytes).digest("hex");
  assert.equal(
    actual,
    asset.sha256,
    `${asset.name} failed its SHA-256 integrity check`,
  );
}

process.stdout.write(`verified ${assets.length} bundled native assets\n`);
