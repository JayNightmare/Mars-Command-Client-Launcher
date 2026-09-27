import type { IntegrityReport, ManifestStatus } from "../types/manifest";

export type LaunchGate = {
	ready: boolean;
	/** Short reason shown on the launch control when blocked. */
	reason: string;
	detail: string;
};

/**
 * The launch control stays locked until the local install provably matches the
 * signed manifest. Mods are pinned by CurseForge id and carry no hash, so they
 * are gated on count rather than content.
 */
export function evaluateLaunch(
	manifest: ManifestStatus | null,
	report: IntegrityReport | null,
	instanceRoot: string | null,
): LaunchGate {
	if (!manifest?.available || !manifest.signatureValid) {
		return {
			ready: false,
			reason: "NO TRUSTED MANIFEST",
			detail: "Mars Command cannot confirm which pack version you should be running.",
		};
	}

	if (!instanceRoot) {
		return {
			ready: false,
			reason: "NO INSTANCE SELECTED",
			detail: "Select your Mars instance folder so it can be verified.",
		};
	}

	if (!report || report.error) {
		return {
			ready: false,
			reason: "NOT VERIFIED",
			detail: report?.error ?? "Run a pack integrity check before launching.",
		};
	}

	const broken =
		report.missingCount + report.corruptCount + report.unreadableCount;
	if (broken > 0) {
		return {
			ready: false,
			reason: `${broken} FILE${broken === 1 ? "" : "S"} OUT OF DATE`,
			detail: "Local files do not match the approved manifest. Sync the pack to continue.",
		};
	}

	if (report.modsPresent !== report.modsExpected) {
		return {
			ready: false,
			reason: "MOD COUNT MISMATCH",
			detail: `Expected ${report.modsExpected} mods, found ${report.modsPresent}.`,
		};
	}

	return {
		ready: true,
		reason: "PACK VERIFIED",
		detail: `Pack ${manifest.packVersion ?? "?"} matches the approved manifest.`,
	};
}
