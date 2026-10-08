import type { CommunityProfile, ProfileInput, ProfileMod } from "../types/community";

export function profileInputError(input: ProfileInput): string | null {
	if (!input.name.trim()) return "A profile name is required.";
	for (const mod of input.mods) {
		if (!mod.name.trim() || !mod.version.trim()) return "Every mod needs a name and version.";
		if (!/^[a-f0-9]{64}$/i.test(mod.sha256)) return "Every mod needs a 64-character SHA-256 checksum.";
		try {
			const url = new URL(mod.sourceUrl);
			if (url.protocol !== "https:" || url.username || url.password) {
				return "Mod source URLs must use HTTPS without credentials.";
			}
		} catch {
			return "Every mod needs a valid HTTPS source URL.";
		}
	}
	return null;
}

export function copyProfileInput(profile: CommunityProfile): ProfileInput {
	return {
		name: `${profile.name} (personal copy)`,
		description: profile.description,
		mods: profile.mods.map((mod: ProfileMod) => ({ ...mod })),
		sourceProfileId: profile.id,
	};
}
