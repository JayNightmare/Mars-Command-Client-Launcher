import {
	Accessibility,
	Bug,
	Download,
	ExternalLink,
	FolderOpen,
	GitBranch,
	Globe,
	Heart,
	Lock,
	RefreshCw,
	Settings2,
	ShieldCheck,
} from "lucide-react";
import { openPath, openUrl } from "@tauri-apps/plugin-opener";
import { useState } from "react";
import { Panel } from "./Panel";
import { PersonalModsPanel } from "./PersonalModsPanel";
import { formatClock } from "../lib/format";
import {
	getClientPreferences,
	updateClientPreferences,
	type ClientPreferences,
} from "../lib/clientPreferences";
import type { PackIntegrityState } from "../hooks/usePackIntegrity";

const PROJECT_URL = "https://mars.nexusgit.info/";
const REPOSITORY_URL =
	"https://github.com/Mars-Command/Client-Launcher";
const ISSUES_URL = `${REPOSITORY_URL}/issues/new`;
const SPONSORS_URL = "https://github.com/sponsors/JayNightmare";

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
	const [preferences, setPreferences] = useState(getClientPreferences);
	const [actionMessage, setActionMessage] = useState<string | null>(null);

	const savePreferences = (updates: Partial<ClientPreferences>) => {
		try {
			setPreferences(updateClientPreferences(updates));
			setActionMessage(null);
		} catch {
			setActionMessage("Could not save this preference.");
		}
	};

	const runQuickAction = (
		label: string,
		action: () => Promise<unknown>,
	) => {
		void action()
			.then(() => setActionMessage(`${label} opened.`))
			.catch((error: unknown) =>
				setActionMessage(
					`Could not open ${label.toLowerCase()}: ${error instanceof Error ? error.message : String(error)}`,
				),
			);
	};

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
							Installation location
							and client actions
						</p>
					</div>
				</header>

				<PersonalModsPanel pack={pack} />

				<div className="grid gap-4 md:grid-cols-2">
					<Panel
						title="Minecraft Installation"
						icon={<FolderOpen size={14} />}
					>
						<div className="space-y-3">
							<p className="text-[11px] leading-4 text-slate-400">
								Setup creates a
								Mars-only game
								directory under
								.minecraft/mars-client
							</p>
							<div className="rounded-lg border border-white/8 bg-black/15 px-3 py-2.5">
								<p className="mb-1 text-[9px] tracking-[0.12em] text-slate-500 uppercase">
									Current
									game
									directory
								</p>
								<p
									className="truncate font-mono text-[11px] text-slate-200"
									title={
										pack.instanceRoot ??
										undefined
									}
								>
									{pack.instanceRoot ??
										"Not set up"}
								</p>
							</div>
							<div className="flex flex-wrap items-center gap-2">
								<button
									type="button"
									onClick={
										pack.chooseInstanceRoot
									}
									disabled={pack.personalModBusy || pack.syncing || pack.busy}
									className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2 text-[11px] text-slate-200 transition hover:bg-white/10"
								>
									<FolderOpen
										size={
											13
										}
									/>
									Choose
									existing
									folder
								</button>
								{pack.instanceRoot ? (
									<button
										type="button"
										onClick={
											pack.clearInstanceRoot
										}
										disabled={pack.personalModBusy || pack.syncing || pack.busy}
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
						title="Update Data"
						icon={<ShieldCheck size={14} />}
					>
						<label className="flex cursor-pointer items-start gap-3 text-[11px]">
							<input
								type="checkbox"
								checked={
									pack.preservePersistentData
								}
								disabled={
									pack.savingPersistentData
								}
								onChange={(
									event,
								) =>
									pack.updatePersistentDataPreference(
										event
											.currentTarget
											.checked,
									)
								}
								className="mt-0.5 h-4 w-4 accent-cyan-300 disabled:cursor-wait"
							/>
							<span>
								<span className="block text-slate-200">
									Preserve
									personal
									data
								</span>
								<span className="mt-1 block leading-4 text-slate-500">
									Keep
									existing
									worlds,
									mod
									configs,
									screenshots,
									server
									list,
									and
									shaders
									during
									pack
									updates.
								</span>
							</span>
						</label>
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
								onClick={
									pack.syncPack
								}
								disabled={
									pack.busy ||
									pack.syncing ||
									pack.personalModBusy
								}
								className="flex w-full items-center justify-between gap-3 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10 disabled:cursor-wait disabled:text-slate-400"
							>
								<span>
									Sync /
									update
									pack
								</span>
								<RefreshCw
									size={
										14
									}
									className={`text-cyan-200 ${pack.syncing ? "animate-spin" : ""}`}
								/>
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

					<Panel
						title="Launch behavior"
						icon={<FolderOpen size={14} />}
					>
						<label className="flex cursor-pointer items-start gap-3 text-[11px]">
							<input
								type="checkbox"
								checked={
									preferences.closeClientAfterGameStart
								}
								onChange={(
									event,
								) =>
									savePreferences(
										{
											closeClientAfterGameStart:
												event
													.currentTarget
													.checked,
										},
									)
								}
								className="mt-0.5 h-4 w-4 accent-cyan-300"
							/>
							<span>
								<span className="block text-slate-200">
									Close
									Mars
									Command
									after
									Minecraft
									starts
								</span>
								<span className="mt-1 block leading-4 text-slate-500">
									Waits up
									to 2
									minutes
									for the
									Mars
									game
									process.
								</span>
							</span>
						</label>
					</Panel>

					<Panel
						title="Accessibility"
						icon={
							<Accessibility
								size={14}
							/>
						}
					>
						<div className="space-y-4">
							<label className="flex cursor-pointer items-start gap-3 text-[11px]">
								<input
									type="checkbox"
									checked={
										preferences.reduceMotion
									}
									onChange={(
										event,
									) =>
										savePreferences(
											{
												reduceMotion:
													event
														.currentTarget
														.checked,
											},
										)
									}
									className="mt-0.5 h-4 w-4 accent-cyan-300"
								/>
								<span>
									<span className="block text-slate-200">
										Reduce
										motion
									</span>
									<span className="mt-1 block leading-4 text-slate-500">
										Reduces
										animation
										and
										transition
										effects
										throughout
										the
										client.
									</span>
								</span>
							</label>
							<label className="flex items-center justify-between gap-3 text-[11px]">
								<span className="text-slate-200">
									Text
									size
								</span>
								<select
									value={
										preferences.textScale
									}
									onChange={(
										event,
									) =>
										savePreferences(
											{
												textScale: event
													.currentTarget
													.value as ClientPreferences["textScale"],
											},
										)
									}
									className="rounded-md border border-white/10 bg-slate-900 px-2 py-1.5 text-[11px] text-slate-200"
								>
									<option value="normal">
										Default
									</option>
									<option value="large">
										Larger
									</option>
								</select>
							</label>
						</div>
					</Panel>
				</div>

				<Panel
					title="Quick actions"
					icon={<ExternalLink size={14} />}
				>
					<div className="grid gap-2 sm:grid-cols-2">
						<button
							type="button"
							disabled={
								!pack.instanceRoot
							}
							onClick={() => {
								if (
									pack.instanceRoot
								)
									runQuickAction(
										"Installation folder",
										() =>
											openPath(
												pack.instanceRoot!,
											),
									);
							}}
							className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:text-slate-500"
						>
							<FolderOpen size={14} />
							<span className="flex-1">
								Open
								installation
								folder
							</span>
						</button>
						<button
							type="button"
							onClick={() =>
								runQuickAction(
									"Bug report",
									() =>
										openUrl(
											ISSUES_URL,
										),
								)
							}
							className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10"
						>
							<Bug size={14} />
							<span className="flex-1">
								Report a bug
							</span>
							<ExternalLink
								size={12}
								className="text-slate-500"
							/>
						</button>
						<button
							type="button"
							onClick={() =>
								runQuickAction(
									"GitHub repository",
									() =>
										openUrl(
											REPOSITORY_URL,
										),
								)
							}
							className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10"
						>
							<GitBranch size={14} />
							<span className="flex-1">
								Visit GitHub
								repository
							</span>
							<ExternalLink
								size={12}
								className="text-slate-500"
							/>
						</button>
						<button
							type="button"
							onClick={() =>
								runQuickAction(
									"Project website",
									() =>
										openUrl(
											PROJECT_URL,
										),
								)
							}
							className="flex items-center gap-2 rounded-lg border border-white/8 bg-white/5 px-3 py-2.5 text-left text-[11px] text-slate-200 transition hover:bg-white/10"
						>
							<Globe size={14} />
							<span className="flex-1">
								Visit project
								website
							</span>
							<ExternalLink
								size={12}
								className="text-slate-500"
							/>
						</button>
						<button
							type="button"
							onClick={() =>
								runQuickAction(
									"GitHub Sponsors",
									() =>
										openUrl(
											SPONSORS_URL,
										),
								)
							}
							className="flex items-center gap-2 rounded-lg border border-emerald-300/15 bg-emerald-300/5 px-3 py-2.5 text-left text-[11px] text-emerald-100 transition hover:bg-emerald-300/10"
						>
							<Heart size={14} />
							<span className="flex-1">
								Fund This
								Project
							</span>
							<ExternalLink
								size={12}
								className="text-emerald-200/60"
							/>
						</button>
					</div>
					<p className="mt-2 text-[10px] leading-4 text-slate-500">
						GitHub handles sponsorships on
						the maintainer's page; Mars
						Command does not collect
						payments.
					</p>
					{actionMessage ? (
						<p
							role="status"
							className="mt-2 text-[10px] text-cyan-200"
						>
							{actionMessage}
						</p>
					) : null}
				</Panel>

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

				{pack.installationMessage ? (
					<p
						role="status"
						className={
							pack.launcherOpened
								? "text-[11px] text-emerald-200"
								: "text-[11px] text-amber-200"
						}
					>
						{pack.installationMessage}
					</p>
				) : null}

				{pack.syncResult ? (
					<Panel
						title="Sync Result"
						icon={<ShieldCheck size={14} />}
					>
						<p
							className={`text-[11px] leading-4 ${pack.syncResult.complete ? "text-emerald-200" : "text-amber-200"}`}
						>
							{pack.syncResult
								.error ??
								(pack.syncResult
									.complete
									? `Pack ${pack.syncResult.packVersion} is up to date.`
									: "Sync stopped with files needing attention.")}
						</p>
						<div className="mt-3 grid grid-cols-2 gap-3 text-[10px] sm:grid-cols-3 xl:grid-cols-6">
							<div>
								<span className="text-slate-500">
									Installed
								</span>
								<p className="mt-0.5 font-mono text-slate-200">
									{
										pack
											.syncResult
											.installedCount
									}
								</p>
							</div>
							<div>
								<span className="text-slate-500">
									Updated
								</span>
								<p className="mt-0.5 font-mono text-slate-200">
									{
										pack
											.syncResult
											.updatedCount
									}
								</p>
							</div>
							<div>
								<span className="text-slate-500">
									Unchanged
								</span>
								<p className="mt-0.5 font-mono text-slate-200">
									{
										pack
											.syncResult
											.unchangedCount
									}
								</p>
							</div>
							<div>
								<span className="text-slate-500">
									Preserved
								</span>
								<p className="mt-0.5 font-mono text-emerald-200">
									{
										pack
											.syncResult
											.preservedCount
									}
								</p>
							</div>
							<div>
								<span className="text-slate-500">
									Conflicts
								</span>
								<p className="mt-0.5 font-mono text-amber-200">
									{
										pack
											.syncResult
											.conflictCount
									}
								</p>
							</div>
							<div>
								<span className="text-slate-500">
									Manual /
									failed
								</span>
								<p className="mt-0.5 font-mono text-slate-200">
									{
										pack
											.syncResult
											.manualCount
									}{" "}
									/{" "}
									{
										pack
											.syncResult
											.failedCount
									}
								</p>
							</div>
						</div>
						{pack.syncResult.issues.length >
						0 ? (
							<ul className="mt-3 max-h-28 space-y-1 overflow-y-auto text-[10px]">
								{pack.syncResult.issues
									.slice(
										0,
										12,
									)
									.map(
										(
											issue,
										) => (
											<li
												className="flex items-start gap-2"
												key={`${issue.path}-${issue.reason}`}
											>
												<span
													className="truncate font-mono text-slate-400"
													title={
														issue.path
													}
												>
													{
														issue.path
													}
												</span>
												<span className="text-slate-500">
													{
														issue.reason
													}
												</span>
											</li>
										),
									)}
							</ul>
						) : null}
					</Panel>
				) : null}
			</div>
		</section>
	);
}

export default SettingsPage;
