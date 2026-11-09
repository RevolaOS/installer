// SPDX-License-Identifier: GPL-3.0-only
import React from "react";
import { useHeadingFocus } from "../hooks/useHeadingFocus";
import { errorDetail } from "../lib/errors";
import { chooseLocalPackage, type PackageInfo, preparePackage } from "../lib/package";
import { appendSessionLog } from "../lib/sessionLog";
import { Button } from "./ui/Button";
import { CheckStatusIndicator } from "./ui/CheckList";
import { HelpDisclosure } from "./ui/HelpDisclosure";
import { InstallerScreen } from "./ui/InstallerScreen";
import { ScreenReveal } from "./ui/ScreenReveal";
import { WarningSymbol } from "./ui/WarningSymbol";

interface IUninstallScreenProps {
  onContinue: (pkg: PackageInfo) => void;
  onBack: () => void;
}

const UninstallScreen: React.FC<IUninstallScreenProps> = ({ onContinue, onBack }) => {
  const { headingRef } = useHeadingFocus();
  const [localPackage, setLocalPackage] = React.useState<PackageInfo | null>(null);
  const [loading, setLoading] = React.useState<"choose" | "prepare" | null>(null);
  const [error, setError] = React.useState("");

  const run = async (action: "choose" | "prepare") => {
    setLoading(action);
    setError("");
    try {
      if (action === "choose") {
        const selected = await chooseLocalPackage();
        if (selected) {
          setLocalPackage(selected);
          appendSessionLog(`Local package loaded: ${selected.filename} (${selected.version}).`);
        }
        return;
      }
      const selected = await preparePackage(null, localPackage);
      appendSessionLog(`Package ready: ${selected.version} (${selected.digest}).`);
      onContinue(selected);
    } catch (error) {
      const detail = errorDetail(error);
      setError(detail);
      appendSessionLog(`Package preparation failed: ${detail}`);
    } finally {
      setLoading(null);
    }
  };

  return (
    <InstallerScreen.Root aria-labelledby="uninstall-title">
      <ScreenReveal>
        <div className="mx-auto mb-8 h-28 w-30" aria-hidden="true">
          <WarningSymbol />
        </div>
        <InstallerScreen.Title id="uninstall-title" ref={headingRef} tabIndex={-1}>
          Remove RevolaOS.
        </InstallerScreen.Title>
      </ScreenReveal>
      <ScreenReveal delay={0.08}>
        <InstallerScreen.Description className="max-w-125">
          This deletes RevolaOS and its extensions, including their settings, and restores Apple’s
          bootloader. Your music and Apple’s firmware stay on your iPod.
        </InstallerScreen.Description>
        {localPackage ? (
          <p className="mt-5 text-[12px] text-muted" role="status">
            Using local package {localPackage.filename} ({localPackage.version}).
          </p>
        ) : (
          <HelpDisclosure label="Use a local package">
            <p>
              The latest RevolaOS package downloads automatically when you continue. It carries the
              tools that remove RevolaOS. This option is intended for development and testing.
            </p>
            <Button.Root
              className="mt-3"
              size="compact"
              disabled={!!loading}
              onClick={() => run("choose")}
            >
              <Button.Label>Choose package…</Button.Label>
            </Button.Root>
          </HelpDisclosure>
        )}
        <div className="mx-auto max-w-125 text-left">
          <div role="status" aria-atomic="true">
            {loading === "prepare" && (
              <p className="mt-4 flex items-center gap-2 text-[13px] text-body">
                <CheckStatusIndicator status="checking" />
                {localPackage ? "Preparing package…" : "Downloading and verifying RevolaOS…"}
              </p>
            )}
          </div>
          <div role="alert" className="mt-3 text-[12px] leading-6 text-[#aa3e36] empty:mt-0">
            {error}
          </div>
        </div>
      </ScreenReveal>
      <ScreenReveal delay={0.16}>
        <InstallerScreen.Actions>
          <Button.Root disabled={!!loading} onClick={onBack}>
            <Button.Label>Back</Button.Label>
          </Button.Root>
          <Button.Root disabled={!!loading} onClick={() => run("prepare")}>
            <Button.Label>{loading === "prepare" ? "Preparing…" : "Continue"}</Button.Label>
          </Button.Root>
        </InstallerScreen.Actions>
      </ScreenReveal>
    </InstallerScreen.Root>
  );
};

export { UninstallScreen };
