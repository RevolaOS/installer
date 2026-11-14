// SPDX-License-Identifier: GPL-3.0-only
import { Dialog } from "@base-ui/react/dialog";
import type React from "react";
import { openExternalUrl } from "../lib/backend";
import { type UpdateState, updateIsBusy } from "../lib/updates";
import { Button } from "./ui/Button";
import { InstallationProgress } from "./ui/InstallationProgress";
import { ReleaseNotes } from "./ui/ReleaseNotes";

interface Props {
  state: UpdateState;
  onCheck: () => void;
  onInstall: () => void;
  onDownload: () => void;
  onLater: () => void;
}

const textButton =
  "cursor-pointer rounded-sm text-[12px] text-muted underline-offset-4 hover:text-ink hover:underline focus-visible:outline-2 focus-visible:outline-focus";

/** Shown as soon as an update is known; stays up through download, install and failure. */
export const InstallerUpdateDialog: React.FC<Props> = ({
  state,
  onCheck,
  onInstall,
  onDownload,
  onLater,
}) => {
  const { status, info, error, progress, dismissed } = state;
  const busy = updateIsBusy(status);
  const failed = status === "error" && !!info?.version;
  const open = !!info?.version && !dismissed && (status === "available" || busy || failed);
  if (!info?.version) return null;
  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next && !busy) onLater();
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 z-40 bg-[#302f2d]/25 backdrop-blur-[2px] transition-opacity duration-200 data-[ending-style]:opacity-0 data-[starting-style]:opacity-0" />
        <Dialog.Popup className="installer-dialog fixed top-1/2 left-1/2 z-50 flex max-h-[min(85dvh,36rem)] w-[min(92vw,30rem)] -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden rounded-2xl text-left outline-none transition-[opacity,transform] duration-200 data-[ending-style]:scale-[0.97] data-[ending-style]:opacity-0 data-[starting-style]:scale-[0.97] data-[starting-style]:opacity-0">
          <div className="px-6 pt-6">
            <Dialog.Title className="text-[17px] font-bold tracking-[-0.02em] text-ink">
              {busy
                ? "Updating the installer"
                : failed
                  ? "Installer update failed"
                  : "Installer update available"}
            </Dialog.Title>
            <Dialog.Description className="mt-1 text-[13px] leading-6 text-body">
              {busy
                ? "The installer restarts when the update is ready."
                : failed
                  ? "Try again or download the installer from the release page."
                  : `Version ${info.version} is available. You have ${info.current_version}.`}
            </Dialog.Description>
          </div>
          {busy ? (
            <div className="px-6 pt-5 pb-6">
              <InstallationProgress
                label="Installer update download"
                value={progress}
                complete={false}
              />
            </div>
          ) : (
            <>
              {error && (
                <p role="alert" className="mt-3 px-6 text-[12px] leading-5 text-[#aa3e36]">
                  {error}
                </p>
              )}
              {(info.notes || info.release_url) && (
                <div className="mt-5 min-h-0 flex-1 overflow-y-auto border-y border-[#dedbd5] bg-[#f8f7f4]/80 px-6 py-4">
                  {info.notes ? (
                    <ReleaseNotes notes={info.notes} />
                  ) : (
                    <p className="text-[13px] text-body">No release notes.</p>
                  )}
                </div>
              )}
              <div className="flex flex-wrap items-center gap-3 px-6 py-4">
                {failed ? (
                  <>
                    <Button.Root size="compact" onClick={onCheck}>
                      <Button.Label>Try again</Button.Label>
                    </Button.Root>
                    <Button.Root size="compact" onClick={onDownload}>
                      <Button.Label>Download manually</Button.Label>
                    </Button.Root>
                  </>
                ) : (
                  <>
                    <Button.Root
                      size="compact"
                      onClick={info.mode === "automatic" ? onInstall : onDownload}
                    >
                      <Button.Label>
                        {info.mode === "automatic" ? "Update and restart" : "Download update"}
                      </Button.Label>
                    </Button.Root>
                    <Button.Root size="compact" onClick={onLater}>
                      <Button.Label>Later</Button.Label>
                    </Button.Root>
                  </>
                )}
                {info.release_url && (
                  <button
                    type="button"
                    className={`ml-auto ${textButton}`}
                    onClick={() => void openExternalUrl(info.release_url ?? "")}
                  >
                    Release page
                  </button>
                )}
              </div>
            </>
          )}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
};
