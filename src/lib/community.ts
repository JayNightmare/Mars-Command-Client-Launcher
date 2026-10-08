import { invoke } from "@tauri-apps/api/core";
import type { CommunityProfile, ProfileInput, ProfilePatch } from "../types/community";

export const communityApi = {
	list: (query: string, mine: boolean) =>
		invoke<{ profiles: CommunityProfile[] }>("community_profiles", { query, mine }),
	create: (input: ProfileInput) =>
		invoke<CommunityProfile>("community_create", { input }),
	update: (id: string, patch: ProfilePatch) =>
		invoke<CommunityProfile>("community_update", { id, patch }),
	remove: (id: string) => invoke<void>("community_delete", { id }),
	submit: (id: string) => invoke<CommunityProfile>("community_submit", { id }),
};
