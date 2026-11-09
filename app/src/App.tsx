// SPDX-License-Identifier: GPL-3.0-only
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import React from "react";
import { AboutScreen } from "./components/AboutScreen";
import { DeviceChecksScreen } from "./components/DeviceChecksScreen";
import { DfuScreen } from "./components/DfuScreen";
import { DisclaimerScreen } from "./components/DisclaimerScreen";
import { FirmwareScreen } from "./components/FirmwareScreen";
import { InstallationScreen } from "./components/InstallationScreen";
import { InstallerUpdates } from "./components/InstallerUpdates";
import { MainScreen } from "./components/MainScreen";
import { SuccessScreen } from "./components/SuccessScreen";
import { UninstallScreen } from "./components/UninstallScreen";
import { UsbPermissionsScreen } from "./components/UsbPermissionsScreen";
import { DebugLog } from "./components/ui/DebugLog";
import { startDeviceChecks } from "./lib/deviceChecks";
import { type FirmwareInfo, startInstallation, startUninstall } from "./lib/installation";
import type { PackageInfo } from "./lib/package";
import { appendSessionLog, getSessionLog, subscribeToSessionLog } from "./lib/sessionLog";
import { getUpdateState, subscribeToUpdates, updateIsBusy } from "./lib/updates";
import "./App.css";

export type Screen =
  | "main"
  | "disclaimer"
  | "firmware"
  | "uninstall"
  | "permissions"
  | "dfu"
  | "checks"
  | "installation"
  | "success"
  | "about";

interface AppProps {
  initial?: {
    screen: Screen;
    firmware: FirmwareInfo | null;
    packageDigest: string;
    localPackage?: PackageInfo;
  };
}

const App: React.FC<AppProps> = ({ initial }) => {
  const updater = React.useSyncExternalStore(subscribeToUpdates, getUpdateState);
  const [screen, setScreen] = React.useState<Screen>(initial?.screen ?? "main");
  const [firmware, setFirmware] = React.useState<FirmwareInfo | null>(initial?.firmware ?? null);
  const [localPackage, setLocalPackage] = React.useState<PackageInfo | null>(
    initial?.localPackage ?? null,
  );
  const [packageDigest, setPackageDigest] = React.useState(initial?.packageDigest ?? "");
  const [mode, setMode] = React.useState<"install" | "uninstall">("install");
  const logs = React.useSyncExternalStore(subscribeToSessionLog, getSessionLog);
  const reducedMotion = useReducedMotion();
  const scrollRef = React.useRef<HTMLElement>(null);

  const begin = () => {
    if (!packageDigest) return;
    if (mode === "uninstall") startUninstall(packageDigest);
    else if (firmware) startInstallation(firmware, packageDigest);
    else return;
    setScreen("installation");
  };
  const packageScreen = mode === "uninstall" ? "uninstall" : "firmware";

  return (
    <main
      ref={scrollRef}
      className="app-surface h-full overflow-x-hidden overflow-y-auto overscroll-none"
    >
      <div
        className={`flex min-h-full flex-col items-center px-6 sm:px-12 ${screen === "dfu" ? "py-8" : "py-14"}`}
      >
        <AnimatePresence mode="wait" onExitComplete={() => scrollRef.current?.scrollTo({ top: 0 })}>
          <motion.div
            key={screen}
            className="my-auto w-full max-w-160 shrink-0"
            initial={{ opacity: 0, y: reducedMotion || screen === "dfu" ? 0 : 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: reducedMotion ? 0 : -8 }}
            transition={{ duration: 0.25 }}
          >
            {screen === "main" ? (
              <MainScreen
                updates={<InstallerUpdates />}
                installDisabled={updateIsBusy(updater.status)}
                onAbout={() => setScreen("about")}
                onInstall={() => {
                  appendSessionLog("New setup started.");
                  setMode("install");
                  setFirmware(null);
                  setLocalPackage(null);
                  setPackageDigest("");
                  setScreen("disclaimer");
                }}
                onUninstall={() => {
                  appendSessionLog("Uninstall setup started.");
                  setMode("uninstall");
                  setPackageDigest("");
                  setScreen("uninstall");
                }}
              />
            ) : screen === "about" ? (
              <AboutScreen onBack={() => setScreen("main")} />
            ) : screen === "disclaimer" ? (
              <DisclaimerScreen
                onContinue={() => {
                  appendSessionLog("Installation disclaimer acknowledged.");
                  setScreen("firmware");
                }}
                onQuit={() => setScreen("main")}
              />
            ) : screen === "firmware" ? (
              <FirmwareScreen
                file={firmware}
                localPackage={localPackage}
                onSelectPackage={setLocalPackage}
                onSelect={(file) => {
                  setFirmware(file);
                  if (file) {
                    appendSessionLog(`Firmware validated: ${file.filename} (${file.version}).`);
                  }
                }}
                onContinue={(digest) => {
                  setPackageDigest(digest);
                  setScreen("permissions");
                }}
                onBack={() => setScreen("disclaimer")}
              />
            ) : screen === "uninstall" ? (
              <UninstallScreen
                onContinue={(digest) => {
                  setPackageDigest(digest);
                  setScreen("permissions");
                }}
                onBack={() => setScreen("main")}
              />
            ) : screen === "permissions" ? (
              <UsbPermissionsScreen
                onContinue={() => {
                  appendSessionLog("Waiting for DFU connection.");
                  setScreen("dfu");
                }}
                onBack={() => setScreen(packageScreen)}
              />
            ) : screen === "dfu" ? (
              <DfuScreen
                onContinue={(device) => {
                  startDeviceChecks(device);
                  setScreen("checks");
                }}
              />
            ) : screen === "checks" ? (
              <DeviceChecksScreen
                onContinue={begin}
                onBack={() => setScreen("dfu")}
                onChangeFirmware={() => {
                  setPackageDigest("");
                  setScreen(packageScreen);
                }}
              />
            ) : screen === "installation" ? (
              <InstallationScreen
                onComplete={() => setScreen("success")}
                onRecover={() => setScreen("dfu")}
              />
            ) : (
              <SuccessScreen onDone={() => setScreen("main")} />
            )}
          </motion.div>
        </AnimatePresence>
      </div>
      <DebugLog entries={logs} />
      {import.meta.env.DEV && !initial && screen === "main" && !updateIsBusy(updater.status) && (
        <a
          href="?dev-panel"
          className="fixed right-3 bottom-3 rounded border border-black/20 bg-white px-3 py-2 text-xs text-black"
        >
          Dev panel
        </a>
      )}
    </main>
  );
};

export default App;
