import { useState } from "react";
import { MARS_SERVER } from "../lib/mars";

export type MissionControlTarget = {
	host: string;
	port: number;
};

const STORAGE_KEY = "mars-command.mission-control-target";

export function isValidMissionControlHost(host: string): boolean {
	return (
		host.length <= 253 &&
		/^[a-zA-Z0-9]+(?:[.-][a-zA-Z0-9]+)*$/.test(host)
	);
}

export function isValidMissionControlPort(port: number): boolean {
	return Number.isInteger(port) && port >= 1 && port <= 65535;
}

function isMissionControlTarget(value: unknown): value is MissionControlTarget {
	if (typeof value !== "object" || value === null) return false;
	const target = value as Partial<MissionControlTarget>;
	return (
		typeof target.host === "string" &&
		isValidMissionControlHost(target.host) &&
		typeof target.port === "number" &&
		isValidMissionControlPort(target.port)
	);
}

function readTarget(): MissionControlTarget {
	try {
		const stored = window.localStorage.getItem(STORAGE_KEY);
		if (stored) {
			const target: unknown = JSON.parse(stored);
			if (isMissionControlTarget(target)) return target;
		}
	} catch {
		return { host: MARS_SERVER.host, port: MARS_SERVER.port };
	}

	return { host: MARS_SERVER.host, port: MARS_SERVER.port };
}

export function useMissionControlConfig() {
	const [target, setTarget] = useState(readTarget);
	const [storageError, setStorageError] = useState<string | null>(null);

	function updateTarget(next: MissionControlTarget) {
		setTarget(next);
		try {
			window.localStorage.setItem(
				STORAGE_KEY,
				JSON.stringify(next),
			);
			setStorageError(null);
		} catch {
			setStorageError(
				"Target applied for this session, but browser storage is unavailable.",
			);
		}
	}

	return { target, updateTarget, storageError };
}
