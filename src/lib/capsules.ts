import { invoke } from "@tauri-apps/api/core";
import type { CapsuleInput } from "../types/capsules";
import { parseCapsule, parseCapsuleList, parseSelection } from "./capsuleValidation";

export const capsuleApi = {
	select: async () => parseSelection(await invoke<unknown>("community_capsule_select")),
	list: async (owner: string) => parseCapsuleList(await invoke<unknown>("community_capsules"), owner),
	reserve: async (owner: string, input: CapsuleInput, idempotencyKey: string) =>
		parseCapsule(await invoke<unknown>("community_capsule_reserve", { input, idempotencyKey }), owner),
	upload: async (owner: string, id: string, selectionId: string, operationId: string) =>
		parseCapsule(await invoke<unknown>("community_capsule_upload", { id, selectionId, operationId }), owner, id),
	status: async (owner: string, id: string) =>
		parseCapsule(await invoke<unknown>("community_capsule_status", { id }), owner, id),
	retry: async (owner: string, id: string) =>
		parseCapsule(await invoke<unknown>("community_capsule_retry", { id }), owner, id),
	withdraw: async (owner: string, id: string) =>
		parseCapsule(await invoke<unknown>("community_capsule_withdraw", { id }), owner, id),
};
