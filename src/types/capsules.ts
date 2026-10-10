export type CapsuleState = "reserved" | "uploading" | "quarantined" | "scan_pending" | "scan_blocked" | "rejected" | "publishable" | "expired" | "withdrawn";

export interface CapsuleInput {
	project: string;
	version: string;
	sourceUrl: string;
}

export interface CapsuleEvidence {
	version: number;
	artifactSha256: string;
	provider: string;
	providerResultId: string;
	policyVersion: string;
	scannedAt: string;
	expiresAt: string;
	verdict: "accepted" | "rejected" | "blocked" | "error";
	summary: string;
}

export interface CapsuleQueue {
	status: "pending" | "leased" | "complete" | "blocked" | "failed";
	attempts: number;
	maxAttempts: number;
	nextAttemptAt: string | null;
	lastError: string | null;
}

export interface Capsule extends CapsuleInput {
	releaseId: string;
	ownerId: string;
	createdAt: string;
	artifactSha256: string | null;
	state: CapsuleState;
	revision: number;
	updatedAt: string;
	evidence: CapsuleEvidence | null;
	queue: CapsuleQueue | null;
	publicDownloadAvailable: false;
}

export interface JarSelection {
	selectionId: string;
	fileName: string;
	size: number;
	sha256: string;
	modIds: string[];
}

export interface UploadProgress {
	operationId: string;
	releaseId: string;
	sentBytes: number;
	totalBytes: number;
}
