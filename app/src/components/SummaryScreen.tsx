// SPDX-License-Identifier: GPL-3.0-only
import React from "react";
import { useHeadingFocus } from "../hooks/useHeadingFocus";
import { openExternalUrl } from "../lib/backend";
import { getCheckState, subscribeToChecks } from "../lib/deviceChecks";
import type { PackageInfo } from "../lib/package";
import { Button } from "./ui/Button";
import { InstallerScreen } from "./ui/InstallerScreen";
import { ReleaseNotes } from "./ui/ReleaseNotes";
import { ScreenReveal } from "./ui/ScreenReveal";

interface ISummaryScreenProps {
  mode: "install" | "uninstall";
  pkg: PackageInfo;
  onConfirm: () => void;
  onBack: () => void;
}

/** The last stop before writing to the iPod: what was found and what will be installed. */
const SummaryScreen: React.FC<ISummaryScreenProps> = ({ mode, pkg, onConfirm, onBack }) => {
  const { headingRef } = useHeadingFocus();
  const identity = React.useSyncExternalStore(subscribeToChecks, getCheckState).report?.identity;
  const install = mode === "install";
  const rows = [
    { label: "iPod", value: identity?.model ?? "Checked" },
    { label: "Apple firmware", value: identity?.recorded_firmware ?? "Supported" },
    {
      label: install ? "RevolaOS" : "Removal tools",
      value: pkg.filename ? `${pkg.version} (local package)` : pkg.version,
    },
  ];

  return (
    <InstallerScreen.Root aria-labelledby="summary-title">
      <ScreenReveal>
        <InstallerScreen.Title id="summary-title" ref={headingRef} tabIndex={-1}>
          {install ? "Ready to install." : "Ready to remove RevolaOS."}
        </InstallerScreen.Title>
        <InstallerScreen.Description className="max-w-125">
          {install
            ? "Keep your iPod connected. Installing takes a few minutes and ends with a restart."
            : "Keep your iPod connected. Removal takes a few minutes and ends with a restart into Apple’s software."}
        </InstallerScreen.Description>
      </ScreenReveal>
      <ScreenReveal delay={0.08}>
        <div className="mx-auto mt-7 max-w-125 overflow-hidden rounded-2xl border border-[#d4d1cb] bg-[#f8f7f4]/90 text-[13px] shadow-[inset_0_1px_0_#ffffff,0_2px_6px_#30282006]">
          <dl className="px-5 py-2">
            {rows.map(({ label, value }) => (
              <div
                key={label}
                className="flex flex-wrap items-center justify-between gap-x-5 gap-y-1 border-b border-[#e2dfd9] py-3 text-left last:border-0"
              >
                <dt className="text-body">{label}</dt>
                <dd className="font-medium text-ink">{value}</dd>
              </div>
            ))}
          </dl>
          {install && (pkg.notes || pkg.release_url) && (
            <section
              aria-labelledby="summary-notes-title"
              className="border-t border-[#d4d1cb] bg-[#eeece7]/70 px-5 py-4 text-left"
            >
              <div className="flex items-baseline justify-between gap-4">
                <h2 id="summary-notes-title" className="font-bold text-ink">
                  What’s new in {pkg.version}
                </h2>
                {pkg.release_url && (
                  <button
                    type="button"
                    className="shrink-0 cursor-pointer rounded-sm text-[12px] text-muted underline-offset-4 hover:text-ink hover:underline focus-visible:outline-2 focus-visible:outline-focus"
                    onClick={() => void openExternalUrl(pkg.release_url ?? "")}
                  >
                    Release page
                  </button>
                )}
              </div>
              {pkg.notes ? (
                <ReleaseNotes notes={pkg.notes} className="mt-2" />
              ) : (
                <p className="mt-2 text-body">No release notes.</p>
              )}
            </section>
          )}
        </div>
      </ScreenReveal>
      <ScreenReveal delay={0.16}>
        <InstallerScreen.Actions className="mt-8">
          <Button.Root onClick={onBack}>
            <Button.Label>Back</Button.Label>
          </Button.Root>
          <Button.Root
            className="min-w-40"
            intent={install ? "neutral" : "danger"}
            onClick={onConfirm}
          >
            <Button.Label>{install ? "Install" : "Remove RevolaOS"}</Button.Label>
          </Button.Root>
        </InstallerScreen.Actions>
      </ScreenReveal>
    </InstallerScreen.Root>
  );
};

export { SummaryScreen };
