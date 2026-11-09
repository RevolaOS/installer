// SPDX-License-Identifier: GPL-3.0-only
import assert from "node:assert/strict";
import { test } from "node:test";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fetchAbout } from "../src/lib/about.ts";

globalThis.window = { crypto: globalThis.crypto };
globalThis.isTauri = true;

test("about returns the installer version and the notice text", async () => {
  mockIPC((command) => {
    assert.equal(command, "about");
    return { version: "1.2.3", notice: "RevolaOS\nGPL-3.0-only." };
  });
  assert.deepEqual(await fetchAbout(), { version: "1.2.3", notice: "RevolaOS\nGPL-3.0-only." });
});
