// SPDX-License-Identifier: GPL-3.0-only
export function errorDetail(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "object" && error !== null && "detail" in error) {
    return String(error.detail);
  }
  return String(error);
}
