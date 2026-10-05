import type { Transmission } from "../types/mars";
import packageJson from "../../package.json";

export const MARS_SERVER = {
	host: "play.nexusgit.info",
	/** SRV records are resolved backend-side; this is the advertised port. */
	port: 25565,
	voiceEndpoint: "voice.nexusgit.info",
	minecraftVersion: "1.21.1",
	loader: "NeoForge",
} as const;

export const STATUS_REFRESH_INTERVAL_MS = 15_000;

// Get the current client build version from package metadata.
export const CLIENT_BUILD = packageJson.version;

/** Local placeholder feed until a signed manifest feed exists. */
export const LOCAL_TRANSMISSIONS: Transmission[] = [
	{
		id: "end-protocol",
		title: "END PROTOCOL remains active",
		body: "Orbital anomaly transit requires Mars Command authorisation.",
		tone: "alert",
	},
	{
		id: "lz-01",
		title: "Landing Zone 01 operational",
		body: "Terraforming progress remains within acceptable fictional limits.",
		tone: "notice",
	},
];

export function describePopulation(
	status: { online: boolean; playersOnline: number | null } | null,
	isFirstCheck: boolean,
): string {
	if (isFirstCheck || !status) {
		return "Establishing uplink to Mars Command.";
	}

	if (!status.online) {
		return "Mission control has lost contact with the colony.";
	}

	const players = status.playersOnline ?? 0;
	if (players <= 0)
		return "No personnel detected. The moon remains observant.";
	if (players === 1) {
		return "One personnel unit is operating without supervision.";
	}
	if (players <= 3)
		return "Small expedition underway. Risk level: acceptable-ish.";
	if (players <= 6) {
		return "Multiple personnel units detected. Safety paperwork pending.";
	}
	return "Population density exceeds Mars Command recommendations.";
}
