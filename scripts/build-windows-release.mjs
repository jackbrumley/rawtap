#!/usr/bin/env node

// Builds the Windows release installers: the per-user setup.exe (NSIS,
// packaging/windows/rawtap.nsi) and the system-wide MSI (WiX 5,
// packaging/windows/rawtap.wxs), from one `cargo build --release`.
// Output lands in target/windows-release/{nsis,msi}, which is the only
// Windows source scripts/package-release.mjs packages from.
//
//   npm run release:windows
//
// Needs NSIS (makensis) and the WiX 5 .NET tool:
//   choco install nsis
//   dotnet tool install --global wix --version 5.0.2

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { VERSION, assertPlainVersion } from "./release-assets.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const PACKAGING = resolve(ROOT, "packaging", "windows");
const OUTPUT_DIR = resolve(ROOT, "target", "windows-release");
const BINARY = resolve(ROOT, "target", "release", "rawtap.exe");
const ICON = resolve(ROOT, "packaging", "icons", "rawtap.ico");
const WIX_VERSION = "5.0.2";

function fail(message) {
  console.error(`\nWindows release build failed: ${message}`);
  process.exit(1);
}

function run(command, args) {
  console.log(`\n$ ${command} ${args.join(" ")}\n`);
  try {
    execFileSync(command, args, { cwd: ROOT, stdio: "inherit" });
  } catch {
    fail(`'${command}' exited with an error (see output above).`);
  }
}

// makensis is on PATH after `choco install nsis` only in new shells, so also
// look in its default folder.
function findMakensis() {
  if (spawnSync("makensis", ["/VERSION"], { stdio: "ignore" }).status === 0) {
    return "makensis";
  }
  const defaultPath = "C:\\Program Files (x86)\\NSIS\\makensis.exe";
  if (existsSync(defaultPath)) {
    return defaultPath;
  }
  fail("NSIS is required: choco install nsis");
}

function checkWix() {
  const result = spawnSync("wix", ["--version"], { encoding: "utf8" });
  const version = result.stdout?.trim() ?? "";
  if (result.status !== 0) {
    fail(`WiX ${WIX_VERSION} is required: dotnet tool install --global wix --version ${WIX_VERSION}`);
  }
  if (!version.startsWith(WIX_VERSION)) {
    fail(`WiX ${WIX_VERSION} is required, found ${version}: dotnet tool update --global wix --version ${WIX_VERSION}`);
  }
}

if (process.platform !== "win32") {
  fail("Windows installers can only be built on Windows.");
}
assertPlainVersion();
const makensis = findMakensis();
checkWix();

run("cargo", ["build", "--release", "--locked"]);

rmSync(OUTPUT_DIR, { recursive: true, force: true });
mkdirSync(resolve(OUTPUT_DIR, "nsis"), { recursive: true });
mkdirSync(resolve(OUTPUT_DIR, "msi"), { recursive: true });

run(makensis, [
  "/V3",
  `/DVERSION=${VERSION}`,
  `/DBINARY=${BINARY}`,
  `/DICON=${ICON}`,
  `/DPATH_HELPER=${resolve(PACKAGING, "path-helper.ps1")}`,
  `/DOUTFILE=${resolve(OUTPUT_DIR, "nsis", `rawtap-${VERSION}-x64-setup.exe`)}`,
  resolve(PACKAGING, "rawtap.nsi"),
]);

run("wix", [
  "build",
  "-arch", "x64",
  "-d", `Version=${VERSION}`,
  "-d", `BinaryFile=${BINARY}`,
  "-d", `IconFile=${ICON}`,
  "-o", resolve(OUTPUT_DIR, "msi", `rawtap-${VERSION}-x64.msi`),
  resolve(PACKAGING, "rawtap.wxs"),
]);

console.log(`\nWindows release installers: ${OUTPUT_DIR}`);
console.log("Next: node scripts/package-release.mjs");
