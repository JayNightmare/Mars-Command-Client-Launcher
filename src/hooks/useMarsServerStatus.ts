import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useRef, useState } from "react";
import type { MinecraftServerStatus } from "../types/mars";

type Options = {
	host: string;
	port: number;
	refreshIntervalMs: number;
};

export type MarsServerStatusState = {
	status: MinecraftServerStatus | null;
	/** True only until the very first result arrives. */
	loading: boolean;
	/** True while any request is in flight, including the first. */
	refreshing: boolean;
	refresh: () => void;
	lastSuccessfulCheck: string | null;
};

function synthesizeFailure(
	host: string,
	port: number,
	error: unknown,
): MinecraftServerStatus {
	return {
		online: false,
		host,
		port,
		playersOnline: null,
		playersMax: null,
		latencyMs: null,
		motd: null,
		versionName: null,
		checkedAt: new Date().toISOString(),
		error: error instanceof Error ? error.message : String(error),
	};
}

export function useMarsServerStatus({
	host,
	port,
	refreshIntervalMs,
}: Options): MarsServerStatusState {
	const [status, setStatus] = useState<MinecraftServerStatus | null>(null);
	const [loading, setLoading] = useState(true);
	const [refreshing, setRefreshing] = useState(false);
	const [lastSuccessfulCheck, setLastSuccessfulCheck] = useState<string | null>(
		null,
	);

	const inFlight = useRef(false);
	const mounted = useRef(true);

	const runCheck = useCallback(async () => {
		if (inFlight.current) return;
		inFlight.current = true;
		setRefreshing(true);

		let result: MinecraftServerStatus;
		try {
			result = await invoke<MinecraftServerStatus>("get_minecraft_status", {
				host,
				port,
			});
		} catch (error) {
			// The command itself never rejects; this covers IPC/permission faults.
			result = synthesizeFailure(host, port, error);
		}

		inFlight.current = false;
		if (!mounted.current) return;

		setStatus(result);
		setLoading(false);
		setRefreshing(false);
		if (result.online) setLastSuccessfulCheck(result.checkedAt);
	}, [host, port]);

	useEffect(() => {
		mounted.current = true;
		let timer: number | undefined;

		const stopPolling = () => {
			if (timer !== undefined) {
				window.clearInterval(timer);
				timer = undefined;
			}
		};

		const startPolling = () => {
			stopPolling();
			timer = window.setInterval(() => {
				void runCheck();
			}, refreshIntervalMs);
		};

		const handleVisibility = () => {
			if (document.hidden) {
				stopPolling();
				return;
			}
			void runCheck();
			startPolling();
		};

		void runCheck();
		if (!document.hidden) startPolling();
		document.addEventListener("visibilitychange", handleVisibility);

		return () => {
			mounted.current = false;
			stopPolling();
			document.removeEventListener("visibilitychange", handleVisibility);
		};
	}, [runCheck, refreshIntervalMs]);

	const refresh = useCallback(() => {
		void runCheck();
	}, [runCheck]);

	return { status, loading, refreshing, refresh, lastSuccessfulCheck };
}
