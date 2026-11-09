// SPDX-License-Identifier: GPL-3.0-only
// GitHub-generated release notes carry boilerplate the installer already provides elsewhere.

/** Drops the generated "What's Changed" heading and the "Full Changelog" compare line. */
export function tidyNotes(notes: string): string {
  return notes
    .split("\n")
    .filter(
      (line) =>
        !/^#{1,3}\s*What['’]s Changed\s*$/i.test(line.trim()) &&
        !/^\*\*Full Changelog\*\*:?\s*\S+$/i.test(line.trim()),
    )
    .join("\n")
    .trim();
}

/** Bare URLs in generated notes read better as what they point at. */
export function linkLabel(href: string, text: string): string {
  if (text !== href) return text;
  const pull = href.match(/\/(?:pull|issues)\/(\d+)$/);
  if (pull) return `#${pull[1]}`;
  if (/\/compare\//.test(href)) return "full changelog";
  try {
    return new URL(href).hostname;
  } catch {
    return text;
  }
}
