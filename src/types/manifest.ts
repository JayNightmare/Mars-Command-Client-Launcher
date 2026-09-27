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
	drift: FileDrift[];
	error: string | null;
};

export type ClientSettings = {
	instanceRoot: string | null;
};
