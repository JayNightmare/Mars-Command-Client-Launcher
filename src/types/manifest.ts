export type ManifestStatus = {
	available: boolean;
	signatureValid: boolean;
	sourceUrl: string;
	packVersion: string | null;
	minecraftVersion: string | null;
	loaderVersion: string | null;
	fileCount: number | null;
	managedBytes: number | null;
	manualDownloadCount: number | null;
	modCount: number | null;
	generatedAt: string | null;
	fetchedAt: string;
	error: string | null;
};

export type FileVerdict =
	| "ok"
	| "missing"
	| "corrupt"
	| "modified"
	| "foreign"
	| "unreadable";

export type FileDrift = {
	path: string;
	verdict: FileVerdict;
};

export type IntegrityReport = {
	root: string;
	checkedAt: string;
	packVersion: string;
	totalFiles: number;
	okCount: number;
	missingCount: number;
	corruptCount: number;
	modifiedCount: number;
	foreignCount: number;
	unreadableCount: number;
	modsExpected: number;
	modsPresent: number;
	modsForeign: number;
	modsFullyVerified: boolean;
	drift: FileDrift[];
	error: string | null;
};

export type SyncIssue = {
	path: string;
	reason: string;
};

export type SyncResult = {
	packVersion: string;
	installedCount: number;
	updatedCount: number;
	unchangedCount: number;
	removedCount: number;
	conflictCount: number;
	manualCount: number;
	failedCount: number;
	complete: boolean;
	issues: SyncIssue[];
	error: string | null;
};

export type ClientSettings = {
	instanceRoot: string | null;
};
