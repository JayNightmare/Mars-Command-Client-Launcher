import assert from "node:assert/strict";
import test from "node:test";
import { canRetry, canUpload, canWithdraw, capsuleInputError, capsuleStatusText, MAX_SUBMISSION_BYTES, mergeCapsule, parseCapsule, parseCapsuleList, parseSelection, validProgress } from "./capsuleValidation.ts";

const hash = "b".repeat(64);
const id = "a".repeat(32);
const capsule = (patch = {}) => ({
	releaseId: id, ownerId: "crew", project: "Mod", version: "1", sourceUrl: "https://example.test/mod",
	createdAt: "2026-10-10T10:00:00Z", updatedAt: "2026-10-10T10:00:00Z", revision: 1,
	artifactSha256: null, state: "reserved", evidence: null, queue: null, publicDownloadAvailable: false, ...patch,
});
const selection = (patch = {}) => ({ selectionId: "native-id", fileName: "mod.jar", size: 100, sha256: hash, modIds: ["mod"], ...patch });

test("valid reservations stay separate from install or public download contracts", () => {
	assert.equal(parseCapsule(capsule(), "crew").state, "reserved");
	assert.deepEqual(parseCapsuleList({ capsules: [capsule()] }, "crew"), [capsule()]);
	assert.equal(canUpload(capsule()), true);
	assert.equal(canWithdraw(capsule()), true);
	assert.equal(canRetry(capsule()), false);
	assert.match(capsuleStatusText.publishable, /still private/);
});

test("metadata rejects empty, overlong and unsafe HTTPS sources", () => {
	assert.equal(capsuleInputError(capsule()), null);
	for (const patch of [{ project: "" }, { version: " ".repeat(10) }, { version: "a".repeat(81) },
		{ sourceUrl: "https://localhost/mod" }, { sourceUrl: "https://example.local/mod" },
		{ sourceUrl: "https://example.test/mod#fragment" }, { sourceUrl: "https://example.test:8443/mod" },
		{ sourceUrl: "https://example.test/mo d" }, { sourceUrl: "https://example.test\\mod" },
		{ sourceUrl: "http://example.test/mod" }, { sourceUrl: "https://user:password@example.test/mod" }]) {
		assert.ok(capsuleInputError(capsule(patch)));
	}
});

test("native selection cancellation, invalid archives/extensions and size boundary", () => {
	assert.equal(parseSelection(null), null);
	assert.equal(parseSelection(selection({ size: MAX_SUBMISSION_BYTES })).size, MAX_SUBMISSION_BYTES);
	for (const patch of [{ size: 0 }, { size: MAX_SUBMISSION_BYTES + 1 }, { fileName: "mod.zip" }, { sha256: "bad" }, { modIds: [] }, { sourcePath: "private-path" }]) {
		assert.throws(() => parseSelection(selection(patch)));
	}
});

test("response guards reject wrong identity, malformed status, revisions and public URLs", () => {
	for (const patch of [{ state: "clean" }, { revision: 0 }, { revision: Number.MAX_SAFE_INTEGER + 1 },
		{ ownerId: "other" }, { artifactSha256: "BAD" }, { publicDownloadAvailable: true },
		{ downloadUrl: "https://example.test/mod.jar" }, { accessToken: "must-not-appear" },
		{ state: "scan_pending" }, { updatedAt: "invalid" }, { evidence: undefined }]) {
		assert.throws(() => parseCapsule(capsule(patch), "crew"));
	}
	assert.throws(() => parseCapsule(capsule(), "crew", "c".repeat(32)));
	assert.throws(() => parseCapsuleList({ capsules: [capsule(), capsule()] }, "crew"));
});

test("blocked scanning offers bounded retry; rejected, expired and withdrawn are terminal", () => {
	const blocked = capsule({ state: "scan_blocked", artifactSha256: hash, queue: {
		status: "blocked", attempts: 1, maxAttempts: 3, nextAttemptAt: null, lastError: "scanning_not_configured",
	} });
	assert.equal(parseCapsule(blocked, "crew").state, "scan_blocked");
	assert.equal(canRetry(blocked), true);
	assert.match(capsuleStatusText.scan_blocked, /trusted scanner.*Private and unpublished/);
	assert.equal(canRetry({ ...blocked, queue: { ...blocked.queue, attempts: 3 } }), false);
	assert.equal(canRetry({ ...blocked, state: "rejected" }), false);
	for (const state of ["withdrawn", "expired"]) {
		assert.equal(canUpload(capsule({ state })), false);
		assert.equal(canWithdraw(capsule({ state })), false);
	}
});

test("evidence must be complete and match the bound digest; publishable is not a download", () => {
	const evidence = { version: 1, artifactSha256: hash, provider: "test", providerResultId: "result", policyVersion: "1",
		scannedAt: "2026-10-10T10:00:00Z", expiresAt: "2026-10-11T10:00:00Z", verdict: "accepted", summary: "Accepted evidence" };
	assert.equal(parseCapsule(capsule({ state: "publishable", artifactSha256: hash, evidence }), "crew").publicDownloadAvailable, false);
	for (const patch of [{ artifactSha256: "c".repeat(64) }, { verdict: "clean" }, { version: 0 }, { providerResultId: "" }]) {
		assert.throws(() => parseCapsule(capsule({ state: "publishable", artifactSha256: hash, evidence: { ...evidence, ...patch } }), "crew"));
	}
	assert.throws(() => parseCapsule(capsule({ state: "publishable", artifactSha256: hash }), "crew"));
});

test("stale revisions do not overwrite status or immutable artifact facts", () => {
	const original = capsule({ state: "scan_pending", revision: 3, artifactSha256: hash });
	assert.deepEqual(mergeCapsule([original], capsule({ revision: 2 })), [original]);
	assert.throws(() => mergeCapsule([original], capsule({ revision: 4, project: "Changed" })));
	assert.throws(() => mergeCapsule([original], capsule({ revision: 4 })));
	assert.equal(mergeCapsule([original], { ...original, revision: 4, state: "scan_blocked" })[0].state, "scan_blocked");
});

test("upload interruption/account switches ignore unrelated progress and require server confirmation", () => {
	const progress = { operationId: "new-login-upload", releaseId: id, sentBytes: 40, totalBytes: 100 };
	assert.equal(validProgress(progress, "new-login-upload", id, 100), true);
	assert.equal(validProgress(progress, "old-login-upload", id, 100), false);
	assert.equal(validProgress({ ...progress, sentBytes: 101 }, "new-login-upload", id, 100), false);
	assert.equal(validProgress({ ...progress, sentBytes: NaN }, "new-login-upload", id, 100), false);
	assert.equal(validProgress(progress, "new-login-upload", "c".repeat(32), 100), false);
	assert.equal(validProgress(progress, "new-login-upload", id, 200), false);
	assert.throws(() => parseCapsuleList({ capsules: [capsule()] }, "different-login"));
});
