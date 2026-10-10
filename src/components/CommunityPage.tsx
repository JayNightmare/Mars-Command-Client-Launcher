import { Users } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CommunityAccount } from "../hooks/useCommunityAccount";
import { communityApi } from "../lib/community";
import { copyProfileInput, profileInputError } from "../lib/communityValidation";
import { ACCOUNT_REFRESH_FAILURE, withSecondaryFailure } from "../lib/accountErrors";
import type { CommunityProfile, ProfileInput, ProfileMod } from "../types/community";
import { AccountPanel } from "./AccountPanel";
import { Panel } from "./Panel";
import { CapsuleSubmissions } from "./CapsuleSubmissions";

const button = "rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs text-slate-200 hover:bg-white/10 disabled:opacity-40";
const field = "w-full rounded-lg border border-white/10 bg-slate-950/70 px-3 py-2 text-xs text-slate-100";
const emptyProfile = (): ProfileInput => ({ name: "", description: "", mods: [] });

export function CommunityPage({ account }: { account: CommunityAccount }) {
	const [mine, setMine] = useState(false);
	const [query, setQuery] = useState("");
	const [profiles, setProfiles] = useState<CommunityProfile[] | null>(null);
	const [busy, setBusy] = useState(false);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState("");
	const [notice, setNotice] = useState("");
	const [editor, setEditor] = useState<ProfileInput | null>(null);
	const [editingId, setEditingId] = useState<string | null>(null);
	const [deleteId, setDeleteId] = useState<string | null>(null);
	const request = useRef(0);
	const identity = useRef(account.user?.id);
	identity.current = account.user?.id;

	const load = useCallback(async () => {
		const current = ++request.current;
		setProfiles(null);
		setLoading(true);
		setError("");
		if (mine && !identity.current) {
			setLoading(false);
			return;
		}
		try {
			const result = await communityApi.list(query, mine);
			if (current === request.current) setProfiles(result.profiles);
		} catch (failure) {
			if (current === request.current) {
				setError(String(failure));
				const failureMessage = await withSecondaryFailure(
					String(failure), account.refresh, ACCOUNT_REFRESH_FAILURE,
				);
				if (current === request.current) setError(failureMessage);
			}
		} finally {
			if (current === request.current) setLoading(false);
		}
	}, [account.refresh, mine, query]);

	useEffect(() => {
		const delay = setTimeout(() => void load(), 250);
		return () => { clearTimeout(delay); request.current++; };
	}, [load, account.user?.id]);

	useEffect(() => {
		setProfiles(null);
		setEditor(null);
		setEditingId(null);
		setDeleteId(null);
		setNotice("");
	}, [account.user?.id]);

	async function mutate(action: () => Promise<unknown>, success: string) {
		if (busy) return;
		const userId = account.user?.id;
		setBusy(true);
		setError("");
		setNotice("");
		try {
			await action();
			if (identity.current !== userId) return;
			setEditor(null);
			setEditingId(null);
			setDeleteId(null);
			setNotice(success);
			await load();
		} catch (failure) {
			if (identity.current !== userId) return;
			setError(String(failure));
			const failureMessage = await withSecondaryFailure(
				String(failure), account.refresh, ACCOUNT_REFRESH_FAILURE,
			);
			if (identity.current === userId) setError(failureMessage);
		} finally {
			setBusy(false);
		}
	}

	function changeMod(index: number, key: keyof ProfileMod, value: string) {
		setEditor((current) => current && ({ ...current, mods: current.mods.map((mod, i) => i === index ? { ...mod, [key]: value } : mod) }));
	}

	return (
		<section className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
			<AccountPanel account={account} />
			<CapsuleSubmissions key={`${account.sessionEpoch}:${account.user?.id ?? "signed-out"}`} account={account} />
			<Panel title="Community profiles" icon={<Users size={14} />}>
				<div className="space-y-3 text-xs text-slate-300">
					<p className="text-slate-400">Browse public metadata or manage private personal profiles. Copying metadata does not download, install, verify, or scan mod files. Verified community file installation is not available in this batch.</p>
					<div className="flex flex-wrap gap-2">
						<button type="button" className={button} aria-pressed={!mine} disabled={busy} onClick={() => setMine(false)}>Public profiles</button>
						<button type="button" className={button} aria-pressed={mine} disabled={busy || !account.user} onClick={() => setMine(true)}>My profiles</button>
						<button type="button" className={button} disabled={busy || loading} onClick={() => void load()}>Refresh</button>
						<button type="button" className={button} disabled={busy || !account.user} onClick={() => { setEditingId(null); setEditor(emptyProfile()); }}>Create private profile</button>
					</div>
					{!mine ? <label className="block">Search public names and descriptions (case-insensitive)<input className={`${field} mt-1`} value={query} onChange={(event) => setQuery(event.currentTarget.value)} /></label> : null}
					{mine && !account.user ? <p>Sign in to load your profiles.</p> : null}
					{loading ? <p role="status">Loading profiles…</p> : null}
					{error ? <p role="alert" className="break-words text-red-300">{error}</p> : null}
					{notice ? <p role="status" className="text-cyan-200">{notice}</p> : null}
					{editor && account.user ? (
						<form className="space-y-3 rounded-lg border border-cyan-300/20 p-3" onSubmit={(event) => {
							event.preventDefault();
							const failure = profileInputError(editor);
							if (failure) { setError(failure); return; }
							void mutate(() => editingId ? communityApi.update(editingId, { name: editor.name, description: editor.description, mods: editor.mods }) : communityApi.create(editor), "Private profile saved. No files were installed.");
						}}>
							<h3 className="font-semibold">{editingId ? "Edit private profile" : "Create private profile"}</h3>
							<label className="block">Name<input required disabled={busy} className={`${field} mt-1`} value={editor.name} onChange={(event) => setEditor({ ...editor, name: event.currentTarget.value })} /></label>
							<label className="block">Description<textarea disabled={busy} className={`${field} mt-1`} value={editor.description} onChange={(event) => setEditor({ ...editor, description: event.currentTarget.value })} /></label>
							<p className="text-slate-500">Mod metadata only. URLs and checksums here are not evidence of a completed file verification or malware scan.</p>
							{editor.mods.map((mod, index) => (
								<fieldset disabled={busy} key={index} className="space-y-2 rounded-lg border border-white/10 p-3">
									<legend>Mod {index + 1}</legend>
									{(["name", "version", "sourceUrl", "sha256"] as const).map((key) => <label key={key} className="block">{({ name: "Mod name", version: "Version", sourceUrl: "HTTPS source URL", sha256: "SHA-256" })[key]}<input required className={`${field} mt-1`} value={mod[key]} onChange={(event) => changeMod(index, key, event.currentTarget.value)} /></label>)}
									<button type="button" className={button} onClick={() => setEditor({ ...editor, mods: editor.mods.filter((_, i) => i !== index) })}>Remove metadata entry</button>
								</fieldset>
							))}
							<div className="flex gap-2">
								<button type="button" className={button} disabled={busy} onClick={() => setEditor({ ...editor, mods: [...editor.mods, { name: "", version: "", sourceUrl: "", sha256: "" }] })}>Add mod metadata</button>
								<button type="submit" className={button} disabled={busy}>Save private profile</button>
								<button type="button" className={button} disabled={busy} onClick={() => setEditor(null)}>Cancel edit</button>
							</div>
						</form>
					) : null}
					{profiles?.length === 0 && !error ? <p>No matching profiles.</p> : null}
					<ul className="space-y-3">
						{profiles?.map((profile) => {
							const owned = profile.owner.id === account.user?.id;
							return (
								<li key={profile.id} className="space-y-2 rounded-lg border border-white/10 p-3">
									<h3 className="text-sm font-semibold text-slate-100">{profile.name}</h3>
									<p className="whitespace-pre-wrap break-words">{profile.description}</p>
									<p className="text-slate-500">{profile.visibility} / {profile.owner.username} / {profile.mods.length} mod metadata entries / updated {profile.updatedAt}</p>
									{profile.sourceProfileId ? <p className="break-all text-slate-500">Independent copy of {profile.sourceProfileId}</p> : null}
									<ul className="space-y-1 text-slate-400">{profile.mods.map((mod, index) => <li key={index} className="break-all">{mod.name} {mod.version} — {mod.sourceUrl} — SHA-256: {mod.sha256}</li>)}</ul>
									<div className="flex flex-wrap gap-2">
										{profile.visibility === "public" ? <button type="button" className={button} disabled={busy || !account.user} onClick={() => void mutate(() => communityApi.create(copyProfileInput(profile)), "Copied metadata to an independent private profile. No mod files were installed.")}>Copy to personal (metadata only)</button> : null}
										{owned && profile.visibility === "private" ? <>
											<button type="button" className={button} disabled={busy} onClick={() => { setEditingId(profile.id); setEditor({ name: profile.name, description: profile.description, mods: profile.mods.map((mod) => ({ ...mod })) }); }}>Edit</button>
											<button type="button" className={button} disabled={busy || profile.mods.length === 0} onClick={() => void mutate(() => communityApi.submit(profile.id), "Submission accepted by the API.")}>Submit for publication</button>
											<p className="w-full text-amber-200">{profile.mods.length === 0 ? "Empty profiles cannot publish." : "Publication requires server-scanned mods; scanning is not configured in this batch. Submission may be rejected."}</p>
										</> : null}
										{owned ? deleteId === profile.id ? <>
											<span>Delete profile metadata?</span>
											<button type="button" className={button} disabled={busy} onClick={() => void mutate(() => communityApi.remove(profile.id), "Profile deleted. Local mod files are unchanged.")}>Confirm delete</button>
											<button type="button" className={button} disabled={busy} onClick={() => setDeleteId(null)}>Keep profile</button>
										</> : <button type="button" className={button} disabled={busy} onClick={() => setDeleteId(profile.id)}>Delete</button> : null}
									</div>
								</li>
							);
						})}
					</ul>
				</div>
			</Panel>
		</section>
	);
}
