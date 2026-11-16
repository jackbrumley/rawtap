#!/usr/bin/env node

// Checks the in-app updater on an installed build: runs it with the hidden
// --update flag, which takes the same path as Update Now (download the
// update script from the site and run it), then waits for the update script
// to finish and checks that it succeeded. The release workflow runs this
// after installing on each platform, with RAWTAP_UPDATE_BASE_URL pointing at
// the tag's docs/ so the tag's own update and install scripts are used, and
// RAWTAP_NO_RELAUNCH=1 so nothing starts afterwards.
//
// It exercises the app's own process launch, not just the scripts, so a
// build whose updater can't start fails the release instead of stranding
// its users.
//
//   node scripts/test-in-app-update.mjs <path to the installed rawtap binary>

import { spawn } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// Linux: rawtap becomes the updater, so it exits when the update ends.
// Windows: it exits once the updater's window is started.
const APP_EXIT_TIMEOUT_MS = 600_000;
// Downloading and installing the release.
const UPDATE_FINISH_TIMEOUT_MS = 600_000;
const STARTED_LINE = "[rawtap-update] started";
const UPDATED_LINE = "[rawtap-update] finished: updated";
const FAILED_LINE = "[rawtap-update] finished: failed";
// RawTap keeps its files in ~/.config/rawtap on every platform.
const UPDATE_LOG = join(homedir(), ".config", "rawtap", "debug", "update.log");

const binary = process.argv[2];
if (!binary) {
  console.error("Usage: node scripts/test-in-app-update.mjs <rawtap binary>");
  process.exit(2);
}
for (const variable of ["RAWTAP_UPDATE_BASE_URL", "RAWTAP_NO_RELAUNCH"]) {
  if (!process.env[variable]) {
    console.error(`${variable} must be set (see the release workflow)`);
    process.exit(2);
  }
}

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

function readLog() {
  try {
    return readFileSync(UPDATE_LOG, "utf8");
  } catch {
    return "";
  }
}

function fail(message) {
  console.error(`FAIL: ${message}`);
  console.error(`--- ${UPDATE_LOG} ---\n${readLog()}`);
  process.exit(1);
}

// A stale log must not pass for this run's.
rmSync(UPDATE_LOG, { force: true });

const app = spawn(binary, ["--update"], { stdio: ["ignore", "inherit", "inherit"] });
const appExit = await Promise.race([
  new Promise((resolve) => app.on("exit", (code, signal) => resolve({ code, signal }))),
  sleep(APP_EXIT_TIMEOUT_MS).then(() => null),
]);
if (!appExit) {
  app.kill();
  fail(`rawtap --update did not exit within ${APP_EXIT_TIMEOUT_MS / 1000}s`);
}
if (appExit.code !== 0) {
  fail(`rawtap --update exited with ${JSON.stringify(appExit)}`);
}
console.log("rawtap --update exited; waiting for the update to finish");

const deadline = Date.now() + UPDATE_FINISH_TIMEOUT_MS;
while (Date.now() < deadline) {
  const log = readLog();
  if (log.includes(UPDATED_LINE)) {
    if (!log.includes(STARTED_LINE)) {
      fail("the update finished, but the update log has no started line");
    }
    console.log(`--- ${UPDATE_LOG} ---\n${log}`);
    console.log("PASS: the in-app updater ran the update script and the update succeeded");
    process.exit(0);
  }
  if (log.includes(FAILED_LINE)) {
    fail("the update script reported a failed update");
  }
  await sleep(2_000);
}
fail(`the update did not finish within ${UPDATE_FINISH_TIMEOUT_MS / 1000}s`);
