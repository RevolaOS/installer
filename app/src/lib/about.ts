// SPDX-License-Identifier: GPL-3.0-only
import { backendAvailable, invoke } from "./backend.ts";

export interface AboutInfo {
  version: string;
  notice: string;
}

export async function fetchAbout(): Promise<AboutInfo> {
  if (!backendAvailable()) throw new Error("Open the desktop installer to view the notice.");
  return invoke<AboutInfo>("about");
}
