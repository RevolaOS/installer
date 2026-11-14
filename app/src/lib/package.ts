// SPDX-License-Identifier: GPL-3.0-only
import { backendAvailable, invoke } from "./backend.ts";

export interface PackageInfo {
  version: string;
  digest: string;
  filename: string | null;
  /** GitHub release page and body; null for local packages. */
  release_url: string | null;
  notes: string | null;
  /** Display names of what the package installs beside the firmware. */
  extensions: string[];
  themes: string[];
  games: string[];
  apps: string[];
}

export async function chooseLocalPackage(): Promise<PackageInfo | null> {
  if (!backendAvailable()) throw new Error("Open the desktop installer to load a package.");
  return invoke<PackageInfo | null>("choose_local_package");
}

/** Without firmware (uninstall) the package must match the supported Apple firmware. */
export async function preparePackage(
  firmwareSha256: string | null,
  localPackage: PackageInfo | null,
): Promise<PackageInfo> {
  if (!backendAvailable()) throw new Error("Open the desktop installer to load a package.");
  return invoke<PackageInfo>("prepare_package", {
    firmwareSha256,
    localDigest: localPackage?.digest ?? null,
  });
}
