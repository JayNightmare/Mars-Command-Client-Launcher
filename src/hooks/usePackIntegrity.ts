import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
	ClientSettings,
	IntegrityReport,
	ManifestStatus,
	SyncResult,
} from "../types/manifest";

export type PackIntegrityState = {
	manifest: ManifestStatus | null;
	report: IntegrityReport | null;
	instanceRoot: string | null;
	installationAction: InstallationAction | null;
	installationMessage: string | null;
	launcherOpened: boolean;
	busy: boolean;
	syncing: boolean;
	syncResult: SyncResult | null;
	/** Refetches and re-verifies the manifest, then rescans if a root is set. */
	refresh: () => void;
	syncPack: () => void;
	setupLauncherInstallation: () => void;
	chooseInstanceRoot: () => void;
	clearInstanceRoot: () => void;
};

export type InstallationAction = "setup" | "update" | "launch" | "blocked";

type LauncherSetupResult = {
	instanceRoot: string;
	syncResult: SyncResult;
	launcherOpened: boolean;
	message: string | null;
};

export function usePackIntegrity(): PackIntegrityState {
	const [manifest, setManifest] = useState<ManifestStatus | null>(null);
	const [report, setReport] = useState<IntegrityReport | null>(null);
	const [instanceRoot, setInstanceRoot] = useState<string | null>(null);
	const [installationAction, setInstallationAction] =
		useState<InstallationAction | null>(null);
	const [installationMessage, setInstallationMessage] = useState<
		string | null
	>(null);
	const [launcherOpened, setLauncherOpened] = useState(false);
	const [busy, setBusy] = useState(false);
	const [syncing, setSyncing] = useState(false);
	const [syncResult, setSyncResult] = useState<SyncResult | null>(null);

	const inFlight = useRef(false);
	const syncInFlight = useRef(false);
	const mounted = useRef(true);

	const runRefresh = useCallback(async (root: string | null) => {
		if (inFlight.current) return;
		inFlight.current = true;
		setBusy(true);

		try {
			const status =
				await invoke<ManifestStatus>(
					"refresh_manifest",
				);
			if (mounted.current) setManifest(status);

			const action = status.available
				? await invoke<InstallationAction>(
						"get_mars_installation_action",
					)
				: null;
			const savedSettings = await invoke<ClientSettings>(
				"get_client_settings",
			);
			const activeRoot = savedSettings.instanceRoot ?? root;
			if (mounted.current) {
				setInstallationAction(action);
				setInstanceRoot(activeRoot);
			}

			const scan =
				status.available && activeRoot
					? await invoke<IntegrityReport>(
							"scan_instance",
						)
					: null;
			if (mounted.current) setReport(scan);
		} catch {
			// IPC-level faults leave the previous state visible.
		} finally {
			inFlight.current = false;
			if (mounted.current) setBusy(false);
		}
	}, []);

	useEffect(() => {
		mounted.current = true;

		void (async () => {
			let root: string | null = null;
			try {
				const settings = await invoke<ClientSettings>(
					"get_client_settings",
				);
				root = settings.instanceRoot;
				if (!root) {
					root = await invoke<string | null>(
						"auto_detect_instance_root",
					);
				}
			} catch {
				root = null;
			}
			if (!mounted.current) return;
			setInstanceRoot(root);
			await runRefresh(root);
		})();

		return () => {
			mounted.current = false;
		};
	}, [runRefresh]);

	const refresh = useCallback(() => {
		void runRefresh(instanceRoot);
	}, [runRefresh, instanceRoot]);

	const syncPack = useCallback(() => {
		if (syncInFlight.current) return;
		syncInFlight.current = true;
		setSyncing(true);
		setSyncResult(null);

		void (async () => {
			try {
				const status =
					await invoke<ManifestStatus>(
						"refresh_manifest",
					);
				if (!mounted.current) return;
				setManifest(status);

				if (
					!status.available ||
					!status.signatureValid
				) {
					setSyncResult({
						packVersion:
							status.packVersion ??
							"",
						installedCount: 0,
						updatedCount: 0,
						unchangedCount: 0,
						removedCount: 0,
						conflictCount: 0,
						manualCount: 0,
						failedCount: 1,
						complete: false,
						issues: [],
						error:
							status.error ??
							"No trusted manifest is available.",
					});
					return;
				}

				if (!instanceRoot) {
					setSyncResult({
						packVersion:
							status.packVersion ??
							"",
						installedCount: 0,
						updatedCount: 0,
						unchangedCount: 0,
						removedCount: 0,
						conflictCount: 0,
						manualCount: 0,
						failedCount: 1,
						complete: false,
						issues: [],
						error: "Choose a Minecraft game folder before syncing.",
					});
					return;
				}

				const result =
					await invoke<SyncResult>(
						"sync_instance",
					);
				if (!mounted.current) return;
				setSyncResult(result);
				const scan =
					await invoke<IntegrityReport>(
						"scan_instance",
					);
				if (mounted.current) setReport(scan);
				const action = await invoke<InstallationAction>(
					"get_mars_installation_action",
				);
				if (mounted.current)
					setInstallationAction(action);
			} catch (error) {
				if (mounted.current) {
					setSyncResult({
						packVersion:
							manifest?.packVersion ??
							"",
						installedCount: 0,
						updatedCount: 0,
						unchangedCount: 0,
						removedCount: 0,
						conflictCount: 0,
						manualCount: 0,
						failedCount: 1,
						complete: false,
						issues: [],
						error:
							error instanceof Error
								? error.message
								: String(error),
					});
				}
			} finally {
				syncInFlight.current = false;
				if (mounted.current) setSyncing(false);
			}
		})();
	}, [instanceRoot, manifest?.packVersion]);

	const chooseInstanceRoot = useCallback(() => {
		void (async () => {
			try {
				const picked = await invoke<string | null>(
					"choose_instance_root",
				);
				if (!picked || !mounted.current) return;
				setInstanceRoot(picked);
				setInstallationMessage(null);
				setSyncResult(null);
				await runRefresh(picked);
			} catch {
				// Cancelled or unavailable picker: keep the current selection.
			}
		})();
	}, [runRefresh]);

	const setupLauncherInstallation = useCallback(() => {
		if (syncInFlight.current) return;
		if (installationAction === "blocked") return;
		syncInFlight.current = true;
		setSyncing(true);
		setSyncResult(null);
		setInstallationMessage(null);
		setLauncherOpened(false);

		void (async () => {
			try {
				if (installationAction === "launch") {
					await invoke(
						"launch_minecraft_installation",
					);
					if (!mounted.current) return;
					setLauncherOpened(true);
					setInstallationMessage(
						"Mars is verified. Continue in Minecraft Launcher.",
					);
					return;
				}

				const result =
					await invoke<LauncherSetupResult>(
						"setup_minecraft_installation",
					);
				if (!mounted.current) return;
				setInstanceRoot(result.instanceRoot);
				setSyncResult(result.syncResult);
				setLauncherOpened(result.launcherOpened);
				setInstallationMessage(result.message);
				const [scan, action] = await Promise.all([
					invoke<IntegrityReport>(
						"scan_instance",
					),
					invoke<InstallationAction>(
						"get_mars_installation_action",
					),
				]);
				if (mounted.current) {
					setReport(scan);
					setInstallationAction(action);
				}
			} catch (error) {
				if (mounted.current) {
					setInstallationAction("update");
					setInstallationMessage(
						error instanceof Error
							? error.message
							: String(error),
					);
				}
			} finally {
				syncInFlight.current = false;
				if (mounted.current) setSyncing(false);
			}
		})();
	}, [installationAction]);

	const clearInstanceRoot = useCallback(() => {
		void (async () => {
			try {
				await invoke("clear_instance_root");
			} finally {
				if (mounted.current) {
					setInstanceRoot(null);
					setInstallationAction("setup");
					setInstallationMessage(null);
					setLauncherOpened(false);
					setReport(null);
					setSyncResult(null);
				}
			}
		})();
	}, []);

	return {
		manifest,
		report,
		instanceRoot,
		installationAction,
		installationMessage,
		launcherOpened,
		busy,
		syncing,
		syncResult,
		refresh,
		syncPack,
		setupLauncherInstallation,
		chooseInstanceRoot,
		clearInstanceRoot,
	};
}
