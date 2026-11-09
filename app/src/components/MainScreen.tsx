// SPDX-License-Identifier: GPL-3.0-only
import type React from "react";
import { useHeadingFocus } from "../hooks/useHeadingFocus";
import { ChoiceButton } from "./ui/ChoiceButton";
import { InstallerScreen } from "./ui/InstallerScreen";

interface IMainScreenProps {
  onInstall: () => void;
  onUninstall: () => void;
  onAbout: () => void;
  installDisabled?: boolean;
  updates: React.ReactNode;
}

const MainScreen: React.FC<IMainScreenProps> = ({
  onInstall,
  onUninstall,
  onAbout,
  installDisabled,
  updates,
}) => {
  const { headingRef } = useHeadingFocus();

  return (
    <InstallerScreen.Root aria-labelledby="main-title">
      <InstallerScreen.Title id="main-title" ref={headingRef} tabIndex={-1}>
        RevolaOS
      </InstallerScreen.Title>
      <InstallerScreen.Actions className="mx-auto mt-9 max-w-100 flex-col gap-3">
        <ChoiceButton
          title="Install/Update RevolaOS"
          description="Install or update RevolaOS on your iPod."
          onClick={onInstall}
          disabled={installDisabled}
        />
        <ChoiceButton
          title="Uninstall RevolaOS"
          description="Remove RevolaOS and restore Apple’s bootloader."
          onClick={onUninstall}
          disabled={installDisabled}
        />
      </InstallerScreen.Actions>
      {updates}
      <button
        type="button"
        onClick={onAbout}
        className="mx-auto mt-3 block cursor-pointer rounded-sm text-[12px] text-muted underline-offset-4 hover:text-ink hover:underline focus-visible:outline-2 focus-visible:outline-focus"
      >
        About
      </button>
    </InstallerScreen.Root>
  );
};

export { MainScreen };
