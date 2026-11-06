#!/usr/bin/env node

// Builds the Linux release bundles (deb, rpm, AppImage) inside the Ubuntu
// 22.04 container defined in packaging/linux/Containerfile, so the
// glibc floor of every Linux artifact is fixed regardless of the host distro.
// Output lands in target/linux-release, which is the only
// Linux source scripts/package-release.mjs packages from.
//
//   npm run release:linux

import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync } from "node:fs";
import { resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const CONTAINER_CONTEXT = resolve(ROOT, "packaging", "linux");
const OUTPUT_DIR = resolve(ROOT, "target", "linux-release");
const IMAGE_TAG = "rawtap-linux-release:ubuntu22.04";
const CACHE_VOLUME = "rawtap-linux-release-cache";

function fail(message) {
  console.error(`\nLinux release build failed: ${message}`);
  process.exit(1);
}

// The container builds with the same pinned Rust as local builds and CI
// (rust-toolchain.toml).
function readPinnedVersions() {
  const toolchain = readFileSync(resolve(ROOT, "rust-toolchain.toml"), "utf8");
  const rustVersion = toolchain.match(/^channel\s*=\s*"([^"]+)"/m)?.[1];
  if (!rustVersion) {
    fail("the pinned Rust version is missing from rust-toolchain.toml.");
  }
  return { RUST_VERSION: rustVersion };
}

function podman(args) {
  console.log(`\n$ podman ${args.join(" ")}\n`);
  try {
    execFileSync("podman", args, { stdio: "inherit" });
  } catch {
    fail(`'podman ${args[0]}' exited with an error (see output above).`);
  }
}

if (process.platform !== "linux") {
  fail("Linux release bundles can only be built on a Linux host.");
}

if (spawnSync("podman", ["--version"], { stdio: "ignore" }).status !== 0) {
  fail("podman is required (Fedora: 'sudo dnf install podman', Debian/Ubuntu/Mint: 'sudo apt install podman').");
}

mkdirSync(OUTPUT_DIR, { recursive: true });

podman([
  "build",
  "--tag", IMAGE_TAG,
  ...Object.entries(readPinnedVersions()).flatMap(([name, value]) => ["--build-arg", `${name}=${value}`]),
  "--file", resolve(CONTAINER_CONTEXT, "Containerfile"),
  CONTAINER_CONTEXT,
]);

// label=disable avoids SELinux relabeling the whole repository for the
// read-only source mount; rootless podman maps container root to the host user,
// so the packages in OUTPUT_DIR are owned by the invoking user.
podman([
  "run",
  "--rm",
  "--security-opt", "label=disable",
  "--volume", `${ROOT}:/src:ro`,
  "--volume", `${OUTPUT_DIR}:/out`,
  "--volume", `${CACHE_VOLUME}:/cache`,
  IMAGE_TAG,
]);

console.log(`\nLinux release bundles: ${OUTPUT_DIR}`);
console.log("Next: node scripts/package-release.mjs");
