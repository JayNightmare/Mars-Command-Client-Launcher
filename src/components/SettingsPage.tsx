import {
	Download,
	FolderOpen,
	Lock,
	RefreshCw,
	Settings2,
	ShieldCheck,
} from "lucide-react";
import { Panel } from "./Panel";
import { formatClock } from "../lib/format";
import type { PackIntegrityState } from "../hooks/usePackIntegrity";

type Props = {
	pack: PackIntegrityState;
	refreshingTelemetry: boolean;
	refreshTelemetry: () => void;
};

export function SettingsPage({
	pack,
	refreshingTelemetry,
	refreshTelemetry,
}: Props) {
	return (
		<section className="min-h-0 flex-1 overflow-y-auto p-4 sm:p-5">
			<div className="mx-auto w-full max-w-4xl space-y-4">
				<header className="mb-5 flex items-center gap-3 border-b border-white/8 pb-4">
					<span className="grid h-9 w-9 place-items-center rounded-xl border border-cyan-200/15 bg-cyan-300/8 text-cyan-200">
						<Settings2 size={17} />
					</span>
					<div>
						<h1 className="text-base font-semibold tracking-wide text-slate-100">
							Client Settings
						</h1>
						<p className="mt-0.5 text-[11px] text-slate-500">
							Instance location and
							client actions
						</p>
					</div>
				</header>

				<div className="grid gap-4 md:grid-cols-2">
					<Panel
						title="Minecraft Game Folder"
						icon={<FolderOpen size={14} />}
					>
						<div className="space-y-3">
							<p className="text-[11px] leading-4 text-slate-400">
								CurseForge's default Instances folder is checked on startup. If Mars Client is not found, choose the game folder that directly contains mods and config.
							</p>

							<div className="rounded-lg border border-white/8 bg-black/15 px-3 py-2.5">
								<p className="mb-1 text-[9px] tracking-[0.12em] text-slate-500 uppercase">
									Current
									location
								</p>
								<p
									className="truncate font-mono text-[11px] text-slate-200"
									title={
										pack.instanceRoot ??
										undefined
									}
								>
									{pack.instanceRoot ??
										"No Minecraft game folder selected"}
								</p>
							</div>

							<div className="flex flex-wrap items-center gap-2">
								<button
									type="button"
									onClick={
										pack.chooseInstanceRoot
									}
									className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2 text-[11px] text-slate-200 transition hover:bg-white/10"
								>
									<FolderOpen
										size={
											13
										}
									/>
									{pack.instanceRoot
										? "Change game folder"
										: "Choose game folder"}
								</button>
								{pack.instanceRoot ? (
									<button
										type="button"
										onClick={
											pack.clearInstanceRoot
										}
										className="rounded-lg px-3 py-2 text-[11px] text-slate-500 transition hover:bg-white/5 hover:text-slate-200"
									>
										Forget
										folder
									</button>
								) : null}
							</div>
						</div>
					</Panel>

					<Panel
						title="Actions"
						icon={<Download size={14} />}
					>
						<div className="space-y-2">
							<button
								type="button"
								onClick={
									refreshTelemetry
								}
								disabled={
									refreshingTelemetry
								}
								className="flex w-full items-center justify-between gap-3 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10 disabled:cursor-wait disabled:text-slate-400"
							>
								<span>
									Refresh
									server
									telemetry
								</span>
								<RefreshCw
									size={
										14
									}
									className={`text-emerald-300 ${refreshingTelemetry ? "animate-spin" : ""}`}
								/>
							</button>

							<button
								type="button"
								onClick={pack.syncPack}
								disabled={pack.busy || pack.syncing}
								className="flex w-full items-center justify-between gap-3 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10 disabled:cursor-wait disabled:text-slate-400"
							>
								<span>Sync / update pack</span>
								<RefreshCw size={14} className={`text-cyan-200 ${pack.syncing ? "animate-spin" : ""}`} />
							</button>

							<button
								type="button"
								disabled
								className="flex w-full cursor-not-allowed items-center justify-between gap-3 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-500"
							>
								<span>
									Repair
									installation
									//
									COMING
									SOON
								</span>
								<Lock
									size={
										14
									}
								/>
							</button>
						</div>
					</Panel>
				</div>

				<Panel
					title="Pack Status"
					icon={<ShieldCheck size={14} />}
				>
					<div className="grid gap-3 text-[11px] sm:grid-cols-3">
						<div>
							<p className="text-[9px] tracking-[0.12em] text-slate-500 uppercase">
								Manifest
							</p>
							<p className="mt-1 text-slate-200">
								{pack.manifest
									?.signatureValid
									? `Trusted · ${pack.manifest.packVersion ?? "version unknown"}`
									: "Not verified"}
							</p>
						</div>
						<div>
							<p className="text-[9px] tracking-[0.12em] text-slate-500 uppercase">
								Last
								verification
							</p>
							<p className="mt-1 font-mono text-slate-200">
								{formatClock(
									pack
										.report
										?.checkedAt,
								)}
							</p>
						</div>
						<div>
							<p className="text-[9px] tracking-[0.12em] text-slate-500 uppercase">
								Result
							</p>
							<p className="mt-1 text-slate-200">
								{pack.report
									?.error ??
									(pack.report
										? `${pack.report.missingCount + pack.report.corruptCount} files need attention`
										: "Not scanned")}
							</p>
						</div>
					</div>
				</Panel>

				{pack.syncResult ? (
					<Panel title="Sync Result" icon={<ShieldCheck size={14} />}>
						<p className={`text-[11px] leading-4 ${pack.syncResult.complete ? "text-emerald-200" : "text-amber-200"}`}>
							{pack.syncResult.error ?? (pack.syncResult.complete
								? `Pack ${pack.syncResult.packVersion} is up to date.`
								: "Sync stopped with files needing attention.")}
						</p>
						<div className="mt-3 grid grid-cols-2 gap-3 text-[10px] sm:grid-cols-5">
							<div><span className="text-slate-500">Installed</span><p className="mt-0.5 font-mono text-slate-200">{pack.syncResult.installedCount}</p></div>
							<div><span className="text-slate-500">Updated</span><p className="mt-0.5 font-mono text-slate-200">{pack.syncResult.updatedCount}</p></div>
							<div><span className="text-slate-500">Unchanged</span><p className="mt-0.5 font-mono text-slate-200">{pack.syncResult.unchangedCount}</p></div>
							<div><span className="text-slate-500">Conflicts</span><p className="mt-0.5 font-mono text-amber-200">{pack.syncResult.conflictCount}</p></div>
							<div><span className="text-slate-500">Manual / failed</span><p className="mt-0.5 font-mono text-slate-200">{pack.syncResult.manualCount} / {pack.syncResult.failedCount}</p></div>
						</div>
						{pack.syncResult.issues.length > 0 ? (
							<ul className="mt-3 max-h-28 space-y-1 overflow-y-auto text-[10px]">
								{pack.syncResult.issues.slice(0, 12).map((issue) => (
									<li className="flex items-start gap-2" key={`${issue.path}-${issue.reason}`}>
										<span className="truncate font-mono text-slate-400" title={issue.path}>{issue.path}</span>
										<span className="text-slate-500">{issue.reason}</span>
									</li>
								))}
							</ul>
						) : null}
					</Panel>
				) : null}
			</div>
		</section>
	);
}

export default SettingsPage;
