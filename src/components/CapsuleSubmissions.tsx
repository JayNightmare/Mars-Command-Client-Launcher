import { listen } from "@tauri-apps/api/event";
import { Upload } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CommunityAccount } from "../hooks/useCommunityAccount";
import { capsuleApi } from "../lib/capsules";
import { canRetry, canUpload, canWithdraw, capsuleInputError, capsuleStatusText, mergeCapsule, validProgress } from "../lib/capsuleValidation";
import { ACCOUNT_REFRESH_FAILURE, withSecondaryFailure } from "../lib/accountErrors";
import type { Capsule, CapsuleInput, JarSelection, UploadProgress } from "../types/capsules";
import { Panel } from "./Panel";

const button = "rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-xs hover:bg-white/10 disabled:opacity-40";
const field = "mt-1 w-full rounded-lg border border-white/10 bg-slate-950/70 px-3 py-2 text-xs";

export function CapsuleSubmissions({ account }: { account: CommunityAccount }) {
	const owner = account.user?.id;
	const [capsules, setCapsules] = useState<Capsule[]>([]);
	const [input, setInput] = useState<CapsuleInput>({ project: "", version: "", sourceUrl: "" });
	const [selection, setSelection] = useState<JarSelection | null>(null);
	const [acknowledged, setAcknowledged] = useState(false);
	const [busy, setBusy] = useState(false);
	const [error, setError] = useState("");
	const [notice, setNotice] = useState("");
	const [progress, setProgress] = useState<UploadProgress | null>(null);
	const [withdrawId, setWithdrawId] = useState<string | null>(null);
	const active = useRef(true);
	const lock = useRef(false);
	const capsuleSnapshot = useRef<Capsule[]>([]);
	const reservation = useRef<{ metadata: string; key: string } | null>(null);

	useEffect(() => {
		active.current = true;
		return () => { active.current = false; };
	}, []);

	const run = useCallback(async (operation: () => Promise<void>) => {
		if (lock.current || !owner) return;
		lock.current = true;
		setBusy(true);
		setError("");
		setNotice("");
		try {
			await operation();
		} catch (failure) {
			if (!active.current) return;
			setError(String(failure));
			const message = await withSecondaryFailure(String(failure), account.refresh, ACCOUNT_REFRESH_FAILURE);
			if (active.current) setError(message);
		} finally {
			if (active.current) { lock.current = false; setBusy(false); }
		}
	}, [owner, account.refresh]);

	const refresh = useCallback(() => run(async () => {
		const result = await capsuleApi.list(owner!);
		if (active.current) {
			const next = result.reduce((all, capsule) => mergeCapsule(all, capsule), capsuleSnapshot.current.filter((c) => result.some((r) => r.releaseId === c.releaseId)));
			capsuleSnapshot.current = next;
			setCapsules(next);
		}
	}), [owner, run]);

	useEffect(() => { if (owner) void refresh(); }, [owner, refresh]);

	function accept(capsule: Capsule, message: string) {
		if (!active.current) return;
		const next = mergeCapsule(capsuleSnapshot.current, capsule);
		capsuleSnapshot.current = next;
		setCapsules(next);
		setNotice(message);
	}

	async function upload(capsule: Capsule) {
		if (!selection || !acknowledged) return;
		const chosen = selection;
		await run(async () => {
			const operationId = crypto.randomUUID();
			setProgress({ operationId, releaseId: capsule.releaseId, sentBytes: 0, totalBytes: chosen.size });
			const unlisten = await listen<unknown>("community-capsule-upload-progress", (event) => {
				if (active.current && validProgress(event.payload, operationId, capsule.releaseId, chosen.size)) {
					const payload = event.payload;
					setProgress((old) => old && payload.sentBytes >= old.sentBytes ? payload : old);
				}
			});
			try {
				if (!active.current) return;
				accept(await capsuleApi.upload(owner!, capsule.releaseId, chosen.selectionId, operationId),
					"Backend-observed bytes are bound to this release. The artifact remains private.");
			} finally {
				unlisten();
				if (active.current) setProgress(null);
			}
		});
	}

	return (
		<Panel title="Private capsule submissions" icon={<Upload size={14} />}>
			<div className="space-y-3 text-xs text-slate-300">
				<p>Submit one NeoForge JAR to private quarantine. This does not install, activate, sign, or publish it. Local Personal Mods, local capsule staging, and the signed base pack remain separate.</p>
				<p className="text-slate-400">Desktop limit: 64 MiB per JAR. Server limits and validation are authoritative (default 10 active submissions; reservations/failed quarantine expire after 7 days). Only submit files you have permission to upload.</p>
				{!owner ? <p>Sign in through the website to reserve a release and view your private submissions.</p> : (
					<>
						<div className="flex gap-2">
							<button className={button} disabled={busy} onClick={() => void refresh()}>Refresh submissions</button>
							<button className={button} disabled={busy} onClick={() => void run(async () => {
								const selected = await capsuleApi.select();
								if (active.current && selected) { setSelection(selected); setAcknowledged(false); }
							})}>Select submission JAR</button>
						</div>
						{selection ? <div className="space-y-2 rounded-lg border border-white/10 p-3">
							<p>{selection.fileName} — {(selection.size / 1024 / 1024).toFixed(2)} MiB — mod IDs: {selection.modIds.join(", ")}</p>
							<p className="break-all text-slate-500">Local preview SHA-256: {selection.sha256}. The backend hashes the streamed bytes independently.</p>
							<label className="flex items-start gap-2"><input type="checkbox" disabled={busy} checked={acknowledged} onChange={(e) => setAcknowledged(e.currentTarget.checked)} />I have permission to upload this file. Metadata checks are not malware scans or installation approval.</label>
						</div> : null}
						<form className="space-y-2 rounded-lg border border-white/10 p-3" onSubmit={(event) => {
							event.preventDefault();
							const normalized = { project: input.project.trim(), version: input.version.trim(), sourceUrl: input.sourceUrl.trim() };
							const problem = capsuleInputError(normalized);
							if (problem) { setError(problem); return; }
							const metadata = JSON.stringify(normalized);
							if (reservation.current?.metadata !== metadata) reservation.current = { metadata, key: crypto.randomUUID() };
							const key = reservation.current.key;
							void run(async () => accept(await capsuleApi.reserve(owner, normalized, key), "Release reserved. Review its metadata and upload the selected JAR below."));
						}}>
							<p>Immutable release metadata. Retrying unchanged metadata reuses the reservation key; edit metadata to start a new reservation.</p>
							<label className="block">Project<input required maxLength={120} disabled={busy} className={field} value={input.project} onChange={(e) => setInput({ ...input, project: e.currentTarget.value })} /></label>
							<label className="block">Version<input required maxLength={80} disabled={busy} className={field} value={input.version} onChange={(e) => setInput({ ...input, version: e.currentTarget.value })} /></label>
							<label className="block">HTTPS source URL<input required type="url" maxLength={2048} disabled={busy} className={field} value={input.sourceUrl} onChange={(e) => setInput({ ...input, sourceUrl: e.currentTarget.value })} /></label>
							<button className={button} disabled={busy} type="submit">Reserve release / retry reservation</button>
						</form>
						{progress ? <div role="status" className="space-y-1">
							<progress className="w-full" max={progress.totalBytes} value={progress.sentBytes} />
							<p>Sending {Math.floor(progress.sentBytes * 100 / progress.totalBytes)}% — waiting for server confirmation. On interruption, refresh before retrying the same reservation.</p>
						</div> : null}
						{!busy && capsules.length === 0 && !error ? <p>No private submissions found.</p> : null}
						<ul className="space-y-3">{capsules.map((capsule) => <li key={capsule.releaseId} className="space-y-2 rounded-lg border border-white/10 p-3">
							<h3 className="font-semibold">{capsule.project} — {capsule.version}</h3>
							<p>{capsuleStatusText[capsule.state]}</p>
							<p className="break-all text-slate-500">Release {capsule.releaseId} · revision {capsule.revision}<br />Source: {capsule.sourceUrl}<br />Backend SHA-256: {capsule.artifactSha256 ?? "Not bound yet"}</p>
							{capsule.queue ? <p>Scan queue: {capsule.queue.status}; attempts {capsule.queue.attempts}/{capsule.queue.maxAttempts}{capsule.queue.nextAttemptAt ? `; next attempt ${capsule.queue.nextAttemptAt}` : ""}{capsule.queue.lastError ? `; ${capsule.queue.lastError}` : ""}</p> : null}
							{capsule.evidence ? <p>Evidence v{capsule.evidence.version}: {capsule.evidence.verdict} ({capsule.evidence.provider}; policy {capsule.evidence.policyVersion}). {capsule.evidence.summary}</p> : null}
							<div className="flex flex-wrap gap-2">
								{canUpload(capsule) ? <button className={button} disabled={busy || !selection || !acknowledged} onClick={() => void upload(capsule)}>Upload selected JAR / retry transfer</button> : null}
								<button className={button} disabled={busy} onClick={() => void run(async () => accept(await capsuleApi.status(owner, capsule.releaseId), "Submission status refreshed."))}>Refresh status</button>
								{canRetry(capsule) ? <button className={button} disabled={busy} onClick={() => void run(async () => accept(await capsuleApi.retry(owner, capsule.releaseId), "Scan retry requested. A trusted provider is still required."))}>Retry blocked scan</button> : null}
								{canWithdraw(capsule) ? <button className={button} disabled={busy} onClick={() => setWithdrawId(capsule.releaseId)}>Withdraw</button> : null}
							</div>
							{withdrawId === capsule.releaseId ? <div className="flex items-center gap-2">
								<span>Withdraw this private release? This cannot be undone.</span>
								<button className={button} disabled={busy} onClick={() => void run(async () => {
									accept(await capsuleApi.withdraw(owner, capsule.releaseId), "Release withdrawn; no artifact was installed.");
									if (active.current) setWithdrawId(null);
								})}>Confirm withdrawal</button>
								<button className={button} disabled={busy} onClick={() => setWithdrawId(null)}>Cancel</button>
							</div> : null}
						</li>)}</ul>
					</>
				)}
				{busy && !progress ? <p role="status">Working…</p> : null}
				{error ? <p role="alert" className="break-words text-red-300">{error}</p> : null}
				{notice ? <p role="status" className="text-cyan-200">{notice}</p> : null}
			</div>
		</Panel>
	);
}
