import type { Capsule, CapsuleInput, JarSelection, UploadProgress } from "../types/capsules.ts";

export const MAX_SUBMISSION_BYTES = 64 * 1024 * 1024;
const states = ["reserved", "uploading", "quarantined", "scan_pending", "scan_blocked", "rejected", "publishable", "expired", "withdrawn"];
const digest = (value: unknown): value is string => typeof value === "string" && /^[a-f0-9]{64}$/.test(value);
const integer = (value: unknown, min = 0): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= min;
const record = (value: unknown): value is Record<string, unknown> => value !== null && typeof value === "object" && !Array.isArray(value);
const text = (value: unknown, max: number): value is string => typeof value === "string" && !!value.trim() && value.length <= max && !/[\u0000-\u001f\u007f]/.test(value);
const timestamp = (value: unknown): value is string => typeof value === "string" && /^\d{4}-\d{2}-\d{2}T/.test(value) && Number.isFinite(Date.parse(value));
const keys = (value: Record<string, unknown>, allowed: string[]) => Object.keys(value).every((key) => allowed.includes(key));

export function capsuleInputError(input: CapsuleInput): string | null {
	if (!text(input.project, 120)) return "Provide a project name of at most 120 characters.";
	if (!text(input.version, 80)) return "Provide a version of at most 80 characters.";
	try {
		const url = new URL(input.sourceUrl);
		if (input.sourceUrl.length > 2048 || /[\\\u0000-\u0020\u007f]/.test(input.sourceUrl)
			|| url.protocol !== "https:" || !url.hostname || url.username || url.password || url.hash || (url.port && url.port !== "443")
			|| url.hostname === "localhost" || /\.(localhost|local)$/.test(url.hostname) || !url.hostname.includes(".")) throw new Error();
	} catch { return "Provide a public, credential-free HTTPS source URL without fragments."; }
	return null;
}

export function parseCapsule(value: unknown, owner: string, id?: string): Capsule {
	const invalid = () => { throw new Error("Invalid private capsule response. Refresh status."); };
	if (!record(value)) return invalid();
	if (!keys(value, ["releaseId", "ownerId", "project", "version", "sourceUrl", "createdAt", "artifactSha256", "state", "revision", "updatedAt", "evidence", "queue", "publicDownloadAvailable"])
		|| typeof value.releaseId !== "string" || !/^[a-f0-9]{32}$/.test(value.releaseId)
		|| (id !== undefined && id !== value.releaseId) || value.ownerId !== owner
		|| !integer(value.revision, 1) || value.publicDownloadAvailable !== false
		|| !timestamp(value.createdAt) || !timestamp(value.updatedAt)
		|| typeof value.state !== "string" || !states.includes(value.state)
		|| capsuleInputError(value as unknown as CapsuleInput)
		|| (value.artifactSha256 !== null && !digest(value.artifactSha256))) return invalid();
	if (["quarantined", "scan_pending", "scan_blocked", "rejected", "publishable"].includes(value.state) && !digest(value.artifactSha256)) return invalid();
	if (value.evidence !== null) {
		const e = value.evidence;
		if (!record(e) || !keys(e, ["version", "artifactSha256", "provider", "providerResultId", "policyVersion", "scannedAt", "expiresAt", "verdict", "summary"])
			|| !integer(e.version, 1) || !digest(e.artifactSha256) || e.artifactSha256 !== value.artifactSha256
			|| !text(e.provider, 120) || !text(e.providerResultId, 256) || !text(e.policyVersion, 120)
			|| !timestamp(e.scannedAt) || !timestamp(e.expiresAt)
			|| typeof e.verdict !== "string" || !["accepted", "rejected", "blocked", "error"].includes(e.verdict)
			|| typeof e.summary !== "string" || !e.summary.trim() || e.summary.length > 4096) return invalid();
	}
	if (value.state === "publishable" && (!record(value.evidence) || value.evidence.verdict !== "accepted")) return invalid();
	if (value.queue !== null) {
		const q = value.queue;
		if (!record(q) || !keys(q, ["status", "attempts", "maxAttempts", "nextAttemptAt", "lastError"])
			|| typeof q.status !== "string" || !["pending", "leased", "complete", "blocked", "failed"].includes(q.status)
			|| !integer(q.attempts) || !integer(q.maxAttempts, 1) || q.attempts > q.maxAttempts
			|| (q.nextAttemptAt !== null && !timestamp(q.nextAttemptAt))
			|| (q.lastError !== null && (typeof q.lastError !== "string" || q.lastError.length > 4096))) return invalid();
	}
	return value as unknown as Capsule;
}

export function parseCapsuleList(value: unknown, owner: string): Capsule[] {
	if (!record(value) || !keys(value, ["capsules"]) || !Array.isArray(value.capsules)) throw new Error("Invalid private capsule list.");
	const capsules = value.capsules.map((capsule) => parseCapsule(capsule, owner));
	if (new Set(capsules.map((c) => c.releaseId)).size !== capsules.length) throw new Error("Duplicate capsule release identity.");
	return capsules;
}

export function parseSelection(value: unknown): JarSelection | null {
	if (value === null) return null;
	if (!record(value) || !keys(value, ["selectionId", "fileName", "size", "sha256", "modIds"])
		|| !text(value.selectionId, 128) || !text(value.fileName, 255) || !/\.jar$/i.test(value.fileName)
		|| !integer(value.size, 1) || value.size > MAX_SUBMISSION_BYTES || !digest(value.sha256)
		|| !Array.isArray(value.modIds) || value.modIds.length === 0 || !value.modIds.every((id) => text(id, 64))) throw new Error("Invalid native JAR selection.");
	return value as unknown as JarSelection;
}

export function validProgress(value: unknown, operationId: string, releaseId: string, size: number): value is UploadProgress {
	return record(value) && value.operationId === operationId && value.releaseId === releaseId
		&& value.totalBytes === size && integer(value.sentBytes) && value.sentBytes <= size;
}

export function canUpload(capsule: Capsule): boolean {
	return capsule.state === "reserved" && capsule.artifactSha256 === null;
}

export function canRetry(capsule: Capsule): boolean {
	return capsule.state === "scan_blocked" && !!capsule.queue && capsule.queue.attempts < capsule.queue.maxAttempts && ["blocked", "failed"].includes(capsule.queue.status);
}

export function canWithdraw(capsule: Capsule): boolean {
	return !["withdrawn", "expired"].includes(capsule.state);
}

export function mergeCapsule(existing: Capsule[], incoming: Capsule): Capsule[] {
	const previous = existing.find((c) => c.releaseId === incoming.releaseId);
	if (previous && incoming.revision < previous.revision) return existing;
	if (previous && (previous.ownerId !== incoming.ownerId || previous.project !== incoming.project
		|| previous.version !== incoming.version || previous.sourceUrl !== incoming.sourceUrl
		|| previous.createdAt !== incoming.createdAt
		|| (previous.artifactSha256 !== null && previous.artifactSha256 !== incoming.artifactSha256))) throw new Error("Capsule immutable facts changed.");
	return [incoming, ...existing.filter((c) => c.releaseId !== incoming.releaseId)];
}

export const capsuleStatusText: Record<Capsule["state"], string> = {
	reserved: "Reserved — select and upload the JAR.",
	uploading: "Server upload in progress — refresh before retrying.",
	quarantined: "Private quarantine — waiting for scan scheduling.",
	scan_pending: "Scan queued or running — no download is available.",
	scan_blocked: "Scan blocked — a trusted scanner may not be configured. Private and unpublished.",
	rejected: "Rejected — private, unpublished, and not installable.",
	publishable: "Eligible for future publication — still private; no public download or installation.",
	expired: "Expired — reserve a new release if you want to submit again.",
	withdrawn: "Withdrawn — this release cannot be uploaded or retried.",
};
