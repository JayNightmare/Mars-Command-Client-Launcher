import assert from "node:assert/strict";
import test from "node:test";
import { ACCOUNT_CLEANUP_FAILURE, ACCOUNT_REFRESH_FAILURE, withSecondaryFailure } from "./accountErrors.ts";

test("successful cleanup preserves the primary login error", async () => {
	let cleaned = false;
	const message = await withSecondaryFailure("Browser could not open.", async () => { cleaned = true; }, ACCOUNT_CLEANUP_FAILURE);
	assert.equal(cleaned, true);
	assert.equal(message, "Browser could not open.");
});

test("failed cleanup retains login error and warns credentials may remain without leaking secondary details", async () => {
	const message = await withSecondaryFailure("Browser could not open.", async () => { throw new Error("secret-device-code"); }, ACCOUNT_CLEANUP_FAILURE);
	assert.ok(message.startsWith("Browser could not open."));
	assert.ok(message.includes("credentials may remain in memory"));
	assert.ok(message.includes("Close the client"));
	assert.ok(!message.includes("secret-device-code"));
});

test("failed account refresh retains community error and reports unconfirmed session safely", async () => {
	const message = await withSecondaryFailure("scanning_not_configured: Submission is unavailable.", async () => { throw new Error("secret-token"); }, ACCOUNT_REFRESH_FAILURE);
	assert.ok(message.startsWith("scanning_not_configured: Submission is unavailable."));
	assert.ok(message.includes("session status could not be confirmed"));
	assert.ok(!message.includes("secret-token"));
});
