export const EM_DASH = "—";

export function formatNumber(value: number | null | undefined): string {
	return typeof value === "number" && Number.isFinite(value)
		? value.toLocaleString()
		: EM_DASH;
}

export function formatPlayers(
	online: number | null | undefined,
	max: number | null | undefined,
): string {
	if (typeof online !== "number" || !Number.isFinite(online)) return EM_DASH;
	if (typeof max !== "number" || !Number.isFinite(max)) {
		return formatNumber(online);
	}
	return `${formatNumber(online)} / ${formatNumber(max)}`;
}

export function formatBytes(value: number | null | undefined): string {
	if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
		return EM_DASH;
	}
	const units = ["B", "KB", "MB", "GB"];
	let size = value;
	let unit = 0;
	while (size >= 1024 && unit < units.length - 1) {
		size /= 1024;
		unit += 1;
	}
	return `${size >= 10 || unit === 0 ? Math.round(size) : size.toFixed(1)} ${units[unit]}`;
}

export function formatLatency(value: number | null | undefined): string {
	return typeof value === "number" && Number.isFinite(value)
		? `${Math.round(value)} ms`
		: EM_DASH;
}

/** Compact wall-clock time, e.g. `14:03:22`. */
export function formatClock(iso: string | null | undefined): string {
	if (!iso) return EM_DASH;
	const date = new Date(iso);
	if (Number.isNaN(date.getTime())) return EM_DASH;
	return date.toLocaleTimeString([], {
		hour: "2-digit",
		minute: "2-digit",
		second: "2-digit",
	});
}

/**
 * Removes Minecraft legacy `§` formatting codes. The Rust backend already
 * strips these, but MOTDs can arrive from other sources later.
 */
export function stripLegacyColorCodes(value: string): string {
	return value.replace(/\u00a7./g, "");
}

/** Collapses a MOTD to a single readable line and caps runaway length. */
export function normalizeMotd(value: string | null | undefined): string | null {
	if (!value) return null;
	const cleaned = stripLegacyColorCodes(value)
		.replace(/\s*\n\s*/g, " · ")
		.replace(/\s{2,}/g, " ")
		.trim();
	if (!cleaned) return null;
	return cleaned.length > 220 ? `${cleaned.slice(0, 219)}…` : cleaned;
}
