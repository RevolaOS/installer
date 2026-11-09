// SPDX-License-Identifier: GPL-3.0-only
import React from "react";
import logo from "../assets/modular-wheel-logo.svg";
import { useHeadingFocus } from "../hooks/useHeadingFocus";
import { type AboutInfo, fetchAbout } from "../lib/about";
import { Button } from "./ui/Button";
import { InstallerScreen } from "./ui/InstallerScreen";

interface IAboutScreenProps {
  onBack: () => void;
}

const AboutScreen: React.FC<IAboutScreenProps> = ({ onBack }) => {
  const { headingRef } = useHeadingFocus();
  const [info, setInfo] = React.useState<AboutInfo | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    fetchAbout()
      .then((value) => {
        if (!cancelled) setInfo(value);
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(String(reason));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <InstallerScreen.Root aria-labelledby="about-title">
      <img src={logo} alt="" width={96} height={96} className="mx-auto mb-6" />
      <InstallerScreen.Title id="about-title" ref={headingRef} tabIndex={-1}>
        RevolaOS Installer
      </InstallerScreen.Title>
      <InstallerScreen.Description>
        {info ? `Version ${info.version}` : (error ?? "Loading…")}
      </InstallerScreen.Description>
      {info && (
        <pre className="installer-well mx-auto mt-7 max-w-135 overflow-x-auto rounded-lg p-5 text-left font-mono text-[12px] leading-[1.7] whitespace-pre-wrap text-body">
          {info.notice}
        </pre>
      )}
      <InstallerScreen.Actions>
        <Button.Root onClick={onBack}>
          <Button.Label>Back</Button.Label>
        </Button.Root>
      </InstallerScreen.Actions>
    </InstallerScreen.Root>
  );
};

export { AboutScreen };
