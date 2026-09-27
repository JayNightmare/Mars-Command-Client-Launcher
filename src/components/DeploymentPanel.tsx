import {
	ArrowUpRight,
	Boxes,
	FolderOpen,
	Gamepad2,
	Lock,
	RefreshCw,
	ShieldCheck,
	ShieldX,
	TriangleAlert,
} from "lucide-react";
import { Panel } from "./Panel";
import { EM_DASH, formatBytes, formatClock, formatNumber } from "../lib/format";
import { evaluateLaunch } from "../lib/launch";
import type { PackIntegrityState } from "../hooks/usePackIntegrity";
import type { FileVerdict, IntegrityReport } from "../types/manifest";

const VERDICT_LABEL: Record<FileVerdict, string> = {
	ok: "Verified",
	missing: "Missing",
	corrupt: "Corrupt",
	modified: "Modified",
	foreign: "Unmanaged",
	unreadable: "Unreadable",
};

const VERDICT_CLASS: Record<FileVerdict, string> = {
	ok: "text-emerald-200",
	missing: "text-red-300",
	corrupt: "text-red-300",
	modified: "text-amber-200",
	foreign: "text-slate-400",
	unreadable: "text-amber-200",
};

function severityOf(report: IntegrityReport | null): "clean" | "warn" | "bad" {
	if (!report || report.error) return "warn";
	if (
		report.missingCount +
			report.corruptCount +
			report.unreadableCount >
		0
	) {
		return "bad";
	}
	return report.modifiedCount + report.foreignCount > 0
		? "warn"
		: "clean";
}

function Stat({ label, value }: { label: string; value: string }) {
	return (
		<div className="min-w-0">
			<dt className="truncate text-[9px] tracking-[0.12em] text-slate-500 uppercase">
				{label}
			</dt>
			<dd className="truncate font-mono text-[11px] text-slate-200">
				{value}
			</dd>
		</div>
	);
}

export function DeploymentPanel({
	manifest,
	report,
	instanceRoot,
	busy,
	refresh,
	chooseInstanceRoot,
	clearInstanceRoot,
}: PackIntegrityState) {
	const severity = severityOf(report);
	const verified =
		manifest?.available === true && manifest.signatureValid;
	const scanned = report !== null && report.error === null;
	const gate = evaluateLaunch(manifest, report, instanceRoot);
	const progress =
		scanned && report.totalFiles > 0
			? Math.round((report.okCount / report.totalFiles) * 100)
			: 0;

	const summary = manifest?.error
		? "Manifest unavailable. Mars Command cannot confirm which files you should be running."
		: !instanceRoot
			? "Select your Mars instance folder to compare local files against the signed manifest."
			: severity === "clean"
				? "All managed files match the approved manifest."
				: severity === "bad"
					? "Local files diverge from the approved manifest. Repair is not available in this build."
					: "Manifest verified. Some files differ but none are critical.";

	return (
		<Panel
			title="Deployment Status"
			icon={<Boxes size={14} />}
			actions={
				<span
					className={`flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[9px] font-semibold ${
						verified
							? "border-emerald-300/20 bg-emerald-400/10 text-emerald-200"
							: "border-white/12 bg-white/5 text-slate-400"
					}`}
				>
					{verified ? (
						<ShieldCheck size={11} />
					) : (
						<ShieldX size={11} />
					)}
					{verified
						? "MANIFEST TRUSTED"
						: "NO TRUSTED MANIFEST"}
				</span>
			}
			bodyClassName="flex flex-col"
		>
			<div className="flex items-end justify-between gap-4">
				<div>
					<p className="text-2xl leading-none font-bold tracking-tight text-slate-100">
						MARS CLIENT
					</p>
					<p className="mt-1.5 font-mono text-[11px] text-cyan-200">
						PACK{" "}
						{manifest?.packVersion ??
							EM_DASH}{" "}
						//{" "}
						{verified
							? "SIGNED"
							: "UNVERIFIED"}
					</p>
				</div>

				<div className="flex items-center gap-1.5">
					<button
						type="button"
						onClick={chooseInstanceRoot}
						className="flex items-center gap-1.5 rounded-lg border border-white/8 bg-white/5 text-[11px] text-slate-200 transition hover:bg-white/10"
					>
						<FolderOpen size={13} />
						{instanceRoot
							? "Change folder"
							: "Select folder"}
					</button>
					<button
						type="button"
						onClick={refresh}
						disabled={busy}
						aria-label="Re-verify"
						className="grid h-full w-[30px] justify-center place-items-center rounded-lg border border-white/8 text-slate-200 transition hover:bg-white/10 disabled:cursor-wait disabled:text-slate-500"
					>
						<RefreshCw
							size={13}
							className={
								busy
									? "animate-spin"
									: ""
							}
						/>
					</button>
				</div>
			</div>

			<div className="mt-4 rounded-xl border border-white/8 bg-black/15 p-3.5">
				<div className="mb-2 flex items-center justify-between gap-3 text-[11px]">
					<span className="text-slate-300">
						Pack integrity
					</span>
					<span
						className={`font-mono ${
							severity === "clean"
								? "text-emerald-200"
								: severity ===
									  "bad"
									? "text-red-300"
									: "text-amber-200"
						}`}
					>
						{scanned
							? `${formatNumber(report.okCount)} / ${formatNumber(report.totalFiles)} VERIFIED`
							: "NOT SCANNED"}
					</span>
				</div>

				<div className="h-1.5 overflow-hidden rounded-full bg-white/8">
					<div
						className={`h-full rounded-full transition-[width] duration-500 ${
							severity === "bad"
								? "bg-gradient-to-r from-red-400 to-rose-400"
								: "bg-gradient-to-r from-cyan-300 to-emerald-300"
						}`}
						style={{
							width: `${progress}%`,
						}}
					/>
				</div>

				<dl className="mt-3 grid grid-cols-4 gap-3">
					<Stat
						label="Managed files"
						value={formatNumber(
							manifest?.fileCount,
						)}
					/>
					<Stat
						label="Mods"
						value={
							scanned
								? `${report.modsPresent} / ${report.modsExpected}`
								: formatNumber(
										manifest?.modCount,
									)
						}
					/>
					<Stat
						label="Managed size"
						value={formatBytes(
							manifest?.managedBytes,
						)}
					/>
					<Stat
						label="Last scan"
						value={formatClock(
							report?.checkedAt,
						)}
					/>
				</dl>

				<p className="mt-3 text-[11px] leading-4 text-slate-400">
					{summary}
				</p>

				{manifest?.manualDownloadCount ? (
					<p className="mt-2 flex items-start gap-1.5 text-[10px] leading-4 text-amber-200/80">
						<TriangleAlert
							size={12}
							className="mt-0.5 shrink-0"
						/>
						{manifest.manualDownloadCount}{" "}
						file
						{manifest.manualDownloadCount ===
						1
							? ""
							: "s"}{" "}
						cannot be downloaded
						automatically and must be
						fetched manually.
					</p>
				) : null}
			</div>

			{instanceRoot ? (
				<div className="mt-2 flex items-center gap-2">
					<p
						className="min-w-0 flex-1 truncate font-mono text-[10px] text-slate-500"
						title={instanceRoot}
					>
						{instanceRoot}
					</p>
					<button
						type="button"
						onClick={clearInstanceRoot}
						className="shrink-0 text-[10px] text-slate-500 transition hover:text-slate-300"
					>
						Forget
					</button>
				</div>
			) : null}

			{scanned && report.drift.length > 0 ? (
				<ul className="mt-3 max-h-24 min-h-0 space-y-1 overflow-y-auto pr-1">
					{report.drift
						.slice(0, 60)
						.map((entry) => (
							<li
								className="flex items-center justify-between gap-3 text-[10px]"
								key={`${entry.verdict}-${entry.path}`}
							>
								<span
									className="truncate font-mono text-slate-400"
									title={
										entry.path
									}
								>
									{
										entry.path
									}
								</span>
								<span
									className={`shrink-0 ${VERDICT_CLASS[entry.verdict]}`}
								>
									{
										VERDICT_LABEL[
											entry
												.verdict
										]
									}
								</span>
							</li>
						))}
				</ul>
			) : null}

			<div className="mt-auto pt-3">
				<p
					className={`mb-2 text-center text-[10px] leading-4 ${
						gate.ready
							? "text-emerald-200/80"
							: "text-amber-200/80"
					}`}
				>
					{gate.detail}
				</p>
				<button
					type="button"
					disabled
					title={
						gate.ready
							? "Pack verified. Launching is not implemented yet."
							: gate.detail
					}
					className={`flex w-full cursor-not-allowed items-center justify-center gap-2.5 rounded-xl border px-4 py-3.5 text-[13px] font-bold tracking-[0.12em] ${
						gate.ready
							? "border-emerald-300/20 bg-emerald-400/10 text-emerald-200/70"
							: "border-white/10 bg-white/5 text-slate-500"
					}`}
				>
					{gate.ready ? (
						<Gamepad2 size={18} />
					) : (
						<Lock size={16} />
					)}
					{gate.ready
						? "LAUNCH SYSTEMS // NOT YET CONFIGURED"
						: `LAUNCH LOCKED // ${gate.reason}`}
					<ArrowUpRight size={15} />
				</button>
			</div>
		</Panel>
	);
}

export default DeploymentPanel;
