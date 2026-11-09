// SPDX-License-Identifier: GPL-3.0-only
import React from "react";
import {
  checkInstallerUpdate,
  dismissUpdate,
  getUpdateState,
  installInstallerUpdate,
  openInstallerDownload,
  showUpdate,
  subscribeToUpdates,
} from "../lib/updates";
import { InstallerUpdate } from "./InstallerUpdate";
import { InstallerUpdateDialog } from "./InstallerUpdateDialog";

export function InstallerUpdates() {
  const state = React.useSyncExternalStore(subscribeToUpdates, getUpdateState);
  React.useEffect(() => {
    if (getUpdateState().status === "idle") void checkInstallerUpdate();
  }, []);
  return (
    <>
      <InstallerUpdate
        state={state}
        onCheck={() => void checkInstallerUpdate()}
        onShow={showUpdate}
      />
      <InstallerUpdateDialog
        state={state}
        onCheck={() => void checkInstallerUpdate()}
        onInstall={() => void installInstallerUpdate()}
        onDownload={() => void openInstallerDownload()}
        onLater={dismissUpdate}
      />
    </>
  );
}
