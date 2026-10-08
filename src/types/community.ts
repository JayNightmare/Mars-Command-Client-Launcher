export interface CommunityUser {
	id: string;
	username: string;
	avatarUrl: string;
	roles: string[];
}

export interface ProfileMod {
	name: string;
	version: string;
	sourceUrl: string;
	sha256: string;
}

export interface CommunityProfile {
	id: string;
	name: string;
	description: string;
	visibility: "private" | "public";
	owner: CommunityUser;
	mods: ProfileMod[];
	sourceProfileId: string | null;
	updatedAt: string;
}

export interface ProfileInput {
	name: string;
	description: string;
	mods: ProfileMod[];
	sourceProfileId?: string;
}

export type ProfilePatch = Partial<Pick<ProfileInput, "name" | "description" | "mods">>;

export interface LoginStart {
	verificationUri: string;
	expiresIn: number;
	pollInterval: number;
}

export interface LoginPoll {
	status: "pending" | "approved" | "expired" | "denied";
	user: CommunityUser | null;
}
