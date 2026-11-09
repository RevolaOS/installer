// SPDX-License-Identifier: GPL-3.0-only
import React from "react";
import { type UpdateState, updateIsBusy } from "../lib/updates";

interface Props {
  state: UpdateState;
  onCheck: () => void;
  onShow: () => void;
}

const textButton =
  "mt-1 cursor-pointer rounded-sm underline-offset-4 hover:text-ink hover:underline focus-visible:outline-2 focus-visible:outline-focus disabled:cursor-default disabled:no-underline";

/** The version line under the main actions; the update itself lives in the dialog. */
export const InstallerUpdate: React.FC<Props> = ({ state, onCheck, onShow }) => {
  const { status, info, error } = state;
  const [manualCheck, setManualCheck] = React.useState(false);
  const showCurrent = manualCheck && status === "current";
  React.useEffect(() => {
    if (!manualCheck || status === "checking") return;
    const timeout = window.setTimeout(() => setManualCheck(false), status === "current" ? 2500 : 0);
    return () => window.clearTimeout(timeout);
  }, [manualCheck, status]);
  if (status === "disabled") return null;
  const available = !!info?.version;
  const checkFailed = status === "error" && !available;
  return (
    <div className="mx-auto mt-7 max-w-100 text-center text-[12px] text-muted" aria-live="polite">
      <p>
        {available
          ? `Installer ${info.version} is available.`
          : checkFailed
            ? "Couldn’t check for installer updates."
            : info
              ? `Installer ${info.current_version}`
              : "Installer"}
      </p>
      {checkFailed && error && <p className="mt-1 break-words text-[#aa3e36]">{error}</p>}
      {available ? (
        <button
          type="button"
          className={textButton}
          disabled={updateIsBusy(status)}
          onClick={onShow}
        >
          Show update
        </button>
      ) : (
        <button
          type="button"
          disabled={status === "checking" || showCurrent}
          onClick={() => {
            setManualCheck(true);
            onCheck();
          }}
          className={textButton}
        >
          {status === "checking"
            ? "Checking for updates…"
            : showCurrent
              ? "Up to date"
              : checkFailed
                ? "Try again"
                : "Check for updates"}
        </button>
      )}
    </div>
  );
};
