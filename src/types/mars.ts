export type MinecraftServerStatus = {
	online: boolean;
	host: string;
	port: number;
	playersOnline: number | null;
	playersMax: number | null;
	latencyMs: number | null;
	motd: string | null;
	versionName: string | null;
	/** RFC 3339 timestamp produced by the Rust backend. */
	checkedAt: string;
	error: string | null;
};

export type ConnectionPhase =
	| "checking"
	| "online"
	| "offline"
	| "reconnecting";

export type Transmission = {
	id: string;
	title: string;
	body: string;
	tone: "alert" | "notice" | "server";
};
