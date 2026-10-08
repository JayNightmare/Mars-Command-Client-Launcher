import assert from "node:assert/strict";
import test from "node:test";
import { copyProfileInput, profileInputError } from "./communityValidation.ts";

test("public copy owns independent metadata and never includes ownership/publication fields", () => {
	const source = {
		id: "public-id", name: "Crew", description: "Collection", visibility: "public",
		owner: { id: "owner", username: "crew", avatarUrl: "", roles: ["admin"] },
		mods: [{ name: "mod", version: "1", sourceUrl: "https://example.test/mod.jar", sha256: "a".repeat(64) }],
		sourceProfileId: null, updatedAt: "2026-10-07",
	};
	const copy = copyProfileInput(source);
	assert.equal(copy.sourceProfileId, source.id);
	assert.equal(copy.visibility, undefined);
	assert.equal(copy.owner, undefined);
	assert.equal(copy.roles, undefined);
	copy.mods[0].name = "edited";
	assert.equal(source.mods[0].name, "mod");
	assert.equal(source.name, "Crew");
});

test("private empty drafts are valid but require a name", () => {
	assert.equal(profileInputError({ name: "Draft", description: "", mods: [] }), null);
	assert.match(profileInputError({ name: " ", description: "", mods: [] }), /name/);
});

test("metadata rejects malformed checksums and unsafe source URLs", () => {
	const mod = { name: "mod", version: "1", sourceUrl: "https://example.test/mod.jar", sha256: "a".repeat(64) };
	const input = { name: "Crew", description: "", mods: [mod] };
	assert.equal(profileInputError(input), null);
	for (const sourceUrl of ["http://example.test/mod.jar", "file:///secret", "https://user:pass@example.test/mod.jar", "bad-url"]) {
		assert.ok(profileInputError({ ...input, mods: [{ ...mod, sourceUrl }] }));
	}
	assert.ok(profileInputError({ ...input, mods: [{ ...mod, sha256: "invalid" }] }));
});
