export type ClientPreferences = {
	closeClientAfterGameStart: boolean;
	reduceMotion: boolean;
	textScale: "normal" | "large";
};

const STORAGE_KEY = "mars-command-client-preferences";
const DEFAULT_PREFERENCES: ClientPreferences = {
	closeClientAfterGameStart: false,
	reduceMotion: false,
	textScale: "normal",
};

function applyPreferences(preferences: ClientPreferences) {
	if (typeof document === "undefined") return;
	const root = document.documentElement;
	root.dataset.reducedMotion = String(preferences.reduceMotion);
	root.dataset.textScale = preferences.textScale;
}

function loadPreferences(): ClientPreferences {
	try {
		const stored = JSON.parse(
			localStorage.getItem(STORAGE_KEY) ?? "{}",
		) as Partial<ClientPreferences> & {
			closeClientAfterLauncherOpen?: unknown;
		};
		const preferences: ClientPreferences = {
			closeClientAfterGameStart:
				typeof stored.closeClientAfterGameStart ===
				"boolean"
					? stored.closeClientAfterGameStart
					: typeof stored.closeClientAfterLauncherOpen ===
						  "boolean"
						? stored.closeClientAfterLauncherOpen
						: DEFAULT_PREFERENCES.closeClientAfterGameStart,
			reduceMotion:
				typeof stored.reduceMotion === "boolean"
					? stored.reduceMotion
					: DEFAULT_PREFERENCES.reduceMotion,
			textScale:
				stored.textScale === "large"
					? "large"
					: "normal",
		};
		if (
			typeof stored.closeClientAfterGameStart !== "boolean" &&
			typeof stored.closeClientAfterLauncherOpen === "boolean"
		) {
			try {
				localStorage.setItem(
					STORAGE_KEY,
					JSON.stringify(preferences),
				);
			} catch {
				// Keep the migrated value in memory if storage is temporarily unavailable.
			}
		}
		return preferences;
	} catch {
		return { ...DEFAULT_PREFERENCES };
	}
}

let preferences = loadPreferences();
applyPreferences(preferences);

export function getClientPreferences(): ClientPreferences {
	return preferences;
}

export function updateClientPreferences(
	updates: Partial<ClientPreferences>,
): ClientPreferences {
	const next = { ...preferences, ...updates };
	localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
	preferences = next;
	applyPreferences(preferences);
	return preferences;
}
