// SPDX-License-Identifier: GPL-3.0-only
import type { DeviceInfo } from "../lib/deviceChecks.ts";
import type { FirmwareInfo } from "../lib/installation.ts";
import type { PackageInfo } from "../lib/package.ts";

export const mockDevice: DeviceInfo = {
  selector: { bus: 1, address: 1 },
  port_path: [1],
  vendor_id: 0x05ac,
  product_id: 0x1223,
  mode: "DFU",
  dfu_candidate: true,
};

export const mockFirmware: FirmwareInfo = {
  filename: "iPod_38.2.0.5.ipsw",
  version: "2.0.5",
  sha256: "mock-firmware-sha256",
};

export const mockPackage: PackageInfo = {
  version: "0.1.1",
  digest: "mock-package-digest",
  filename: null,
  release_url: "https://github.com/RevolaOS/revola/releases/tag/v0.1.1",
  notes: [
    "## What's Changed",
    "* feat(core): shrink the artwork cache when the library import needs heap by @paviro in https://github.com/RevolaOS/revola/pull/12",
    "* fix(loader): start extensions in manifest order",
    "",
    "**Full Changelog**: https://github.com/RevolaOS/revola/compare/v0.1.0...v0.1.1",
  ].join("\n"),
  extensions: [
    "Appearance",
    "Channels",
    "Chapter Marks",
    "Chapter Menu",
    "Clicker Modes",
    "Compressor",
    "Cover Grid",
    "Crossfeed",
    "Equalizer",
    "EU Volume Limit",
    "Fast Seek",
    "File Info",
    "Homebrew",
    "iAP Auth Bypass",
    "Long Audio Fix",
    "Playback Queue",
    "Progress Indicator",
    "USB Debug",
  ],
  themes: ["Flat"],
  games: ["2048"],
  apps: ["Dice"],
};

export const mockUpdateNotes = [
  "## What's Changed",
  "* feat(app): show installer and firmware release notes",
  "* fix(device): retry DFU detection after a USB reset",
].join("\n");
