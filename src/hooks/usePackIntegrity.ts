import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type {
	ClientSettings,
	IntegrityReport,
	ManifestStatus,
} from "../types/manifest";

export type PackIntegrityState = {
	manifest: ManifestStatus | null;
	report: IntegrityReport | null;
	instanceRoot: string | null;
	busy: boolean;
	/** Refetches and re-verifies the manifest, then rescans if a root is set. */
	refresh: () => void;
	chooseInstanceRoot: () => void;
	clearInstanceRoot: () => void;
};

export function usePackIntegrity(): PackIntegrityState {
	const [manifest, setManifest] = useState<ManifestStatus | null>(null);
	const [report, setReport] = useState<IntegrityReport | null>(null);
	const [instanceRoot, setInstanceRoot] = useState<string | null>(null);
	const [busy, setBusy] = useState(false);

	const inFlight = useRef(false);
	const mounted = useRef(true);

	const runRefresh = useCallback(async (root: string | null) => {
		if (inFlight.current) return;
		inFlight.current = true;
		setBusy(true);

		try {
			const status = await invoke<ManifestStatus>("refresh_manifest");
			if (mounted.current) setManifest(status);

			// Scanning is pointless without both a trusted manifest and a root.
			const scan =
				status.available && root
					? await invoke<IntegrityReport>("scan_instance")
					: null;
			if (mounted.current) setReport(scan);
		} catch {
			// IPC-level faults leave the previous state visible rather than
			// blanking the panel; the manifest status already carries detail.
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
				const settings = await invoke<ClientSettings>("get_client_settings");
				root = settings.instanceRoot;
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

	const chooseInstanceRoot = useCallback(() => {
		void (async () => {
			try {
				const picked = await invoke<string | null>("choose_instance_root");
				if (!picked || !mounted.current) return;
				setInstanceRoot(picked);
				await runRefresh(picked);
			} catch {
				// Cancelled or unavailable picker: keep the current selection.
			}
		})();
	}, [runRefresh]);

	const clearInstanceRoot = useCallback(() => {
		void (async () => {
			try {
				await invoke("clear_instance_root");
			} finally {
				if (mounted.current) {
					setInstanceRoot(null);
					setReport(null);
				}
			}
		})();
	}, []);

	return {
		manifest,
		report,
		instanceRoot,
		busy,
		refresh,
		chooseInstanceRoot,
		clearInstanceRoot,
	};
}
