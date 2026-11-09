// SPDX-License-Identifier: GPL-3.0-only
import assert from "node:assert/strict";
import { test } from "node:test";
import { linkLabel, tidyNotes } from "../src/lib/releaseNotes.ts";

test("generated boilerplate is removed and written notes stay intact", () => {
  const generated = [
    "## What's Changed",
    "* feat(core): shrink the artwork cache by @paviro in https://github.com/RevolaOS/revola/pull/12",
    "",
    "**Full Changelog**: https://github.com/RevolaOS/revola/compare/v0.1.0...v0.1.1",
  ].join("\n");
  assert.equal(
    tidyNotes(generated),
    "* feat(core): shrink the artwork cache by @paviro in https://github.com/RevolaOS/revola/pull/12",
  );
  const written =
    "## Fixes\n\n- Playback no longer stalls after sleep.\n\n## Known issues\n\n- None.";
  assert.equal(tidyNotes(written), written);
});

test("bare links are labelled by what they point at", () => {
  assert.equal(
    linkLabel(
      "https://github.com/RevolaOS/revola/pull/12",
      "https://github.com/RevolaOS/revola/pull/12",
    ),
    "#12",
  );
  assert.equal(
    linkLabel(
      "https://github.com/RevolaOS/revola/issues/3",
      "https://github.com/RevolaOS/revola/issues/3",
    ),
    "#3",
  );
  assert.equal(
    linkLabel(
      "https://github.com/RevolaOS/revola/compare/a...b",
      "https://github.com/RevolaOS/revola/compare/a...b",
    ),
    "full changelog",
  );
  assert.equal(linkLabel("https://example.org/x", "https://example.org/x"), "example.org");
  assert.equal(linkLabel("https://example.org/x", "the docs"), "the docs");
});
