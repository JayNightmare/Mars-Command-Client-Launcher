import { Puzzle, Upload } from "lucide-react";
import { useEffect, useState } from "react";
import type { PackIntegrityState } from "../hooks/usePackIntegrity";
import { formatBytes } from "../lib/format";
import { Panel } from "./Panel";

export function PersonalModsPanel({ pack }: { pack: PackIntegrityState }) {
	const [acknowledged, setAcknowledged] = useState(false);
	const [removing, setRemoving] = useState<string | null>(null);
	const preview = pack.personalModPreview;
	const disabled = pack.personalModBusy || pack.syncing || pack.busy;
	const trusted = pack.manifest?.available && pack.manifest.signatureValid;

	useEffect(() => setAcknowledged(false), [preview]);
	useEffect(() => setRemoving(null), [pack.report, pack.personalModMessage]);

	return (
		<Panel title="Personal Mods" icon={<Puzzle size={14} />}>
			<div className="space-y-3 text-[11px]">
				<p className="leading-4 text-slate-400">
					Add local NeoForge JARs (maximum 64 MiB each, 128 personal JARs per instance) to your versioned Mars instance.
					Files stay on this device. Personal mods are unsigned and never count as signed pack files.
					Close Minecraft before adding, removing, or updating mods.
				</p>
				<p className="break-all font-mono text-slate-500">{pack.instanceRoot ?? "Set up Mars first."}</p>
				<button type="button" disabled={disabled || !trusted || !pack.instanceRoot}
					onClick={pack.choosePersonalMod}
					className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2 text-slate-200 hover:bg-white/10 disabled:opacity-40">
					<Upload size={13} /> {pack.personalModBusy ? "Working..." : "Select local mod JAR"}
				</button>
				{!trusted ? <p className="text-amber-200">A trusted signed manifest is required to manage personal mods.</p> : null}
				{pack.personalModMessage ? <p role="status" className="break-words text-amber-200">{pack.personalModMessage}</p> : null}
				{preview ? (
					<div className="space-y-3 rounded-lg border border-amber-200/20 bg-black/15 p-3">
						<p className="font-semibold text-slate-200">{preview.file.fileName} ({formatBytes(preview.file.size)})</p>
						<p className="text-slate-400">Mod IDs: {preview.file.modIds.join(", ")} // Pack {preview.packVersion}</p>
						<ul className="list-disc space-y-1 pl-4 text-amber-200">
							{preview.warnings.map((warning, index) => <li key={index}>{warning}</li>)}
						</ul>
						<label className="flex items-start gap-2 text-slate-200">
							<input type="checkbox" checked={acknowledged} disabled={disabled}
								onChange={(event) => setAcknowledged(event.currentTarget.checked)} className="accent-cyan-300" />
							I trust this source and accept these compatibility and runtime risks.
						</label>
						<div className="flex gap-2">
							<button type="button" disabled={disabled || !acknowledged} onClick={pack.installPersonalMod}
								className="rounded-lg bg-cyan-300/15 px-3 py-2 text-cyan-200 disabled:opacity-40">Install personal mod</button>
							<button type="button" disabled={disabled} onClick={pack.cancelPersonalMod}
								className="rounded-lg px-3 py-2 text-slate-400">Cancel</button>
						</div>
					</div>
				) : null}
				<p className="text-slate-500">Tracked personal mods: {pack.report?.personalMods.length ?? 0}. Updates preserve these independently of the personal-data setting.</p>
				<ul className="space-y-2">
					{pack.report?.personalMods.map((entry) => (
						<li key={entry.file.fileName} className="rounded-lg border border-white/8 p-3">
							<div className="flex flex-wrap items-center justify-between gap-2">
								<span className="break-all text-slate-200">{entry.file.fileName}</span>
								<span className={entry.status === "installed" ? "text-cyan-200" : "text-amber-200"}>{entry.status === "installed" ? "Local checksum matches (unsigned)" : entry.status}</span>
							</div>
							<p className="mt-1 text-slate-500">{entry.file.modIds.join(", ")} / {formatBytes(entry.file.size)}</p>
							{entry.message ? <p className="mt-1 text-amber-200">{entry.message}</p> : null}
							{removing === entry.file.fileName ? (
								<div className="mt-2 flex items-center gap-2 text-slate-300">
									Remove from this instance?
									<button type="button" disabled={disabled || !trusted} onClick={() => pack.removePersonalMod(entry.file.fileName)} className="px-2 py-1 text-red-300">Confirm removal</button>
									<button type="button" disabled={disabled} onClick={() => setRemoving(null)} className="px-2 py-1">Cancel</button>
								</div>
							) : (
								<button type="button" disabled={disabled || !trusted} onClick={() => setRemoving(entry.file.fileName)} className="mt-2 px-2 py-1 text-red-300 disabled:opacity-40">Remove</button>
							)}
						</li>
					))}
				</ul>
				{pack.report?.error ? <p role="alert" className="text-red-300">{pack.report.error}</p> : null}
			</div>
		</Panel>
	);
}
