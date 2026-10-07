import {
	Boxes,
	Download,
	ExternalLink,
	Lock,
	LoaderCircle,
	RefreshCw,
	ShieldCheck,
	ShieldX,
	TriangleAlert,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import { Panel } from "./Panel";
import { EM_DASH, formatBytes, formatClock, formatNumber } from "../lib/format";
import type { PackIntegrityState } from "../hooks/usePackIntegrity";
import type { FileVerdict, IntegrityReport } from "../types/manifest";

type InstallationRepairStatus = {
	installedVersion: string;
	latestVersion: string | null;
	assetUrl: string | null;
	releaseUrl: string | null;
	updateAvailable: boolean;
	platform: string;
	message: string | null;
};

const RELEASES_PAGE_URL =
	"https://github.com/JayNightmare/Mars-Command-Client-Launcher/releases";

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
			0 ||
		report.modsPresent !== report.modsExpected
		|| report.personalMods.some((entry) => entry.status !== "installed")
	) {
		return "bad";
	}
	return report.modifiedCount + report.foreignCount + report.modsForeign >
		0
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
	installationAction,
	syncing,
	personalModBusy,
	installationMessage,
	launcherOpened,
	setupLauncherInstallation,
}: PackIntegrityState) {
	const [repairStatus, setRepairStatus] =
		useState<InstallationRepairStatus | null>(null);
	const [repairChecking, setRepairChecking] = useState(true);
	const [repairCheckError, setRepairCheckError] = useState<string | null>(
		null,
	);
	const [repairOpenError, setRepairOpenError] = useState<string | null>(
		null,
	);
	const [repairCheckAttempt, setRepairCheckAttempt] = useState(0);
	const [openingRepair, setOpeningRepair] = useState(false);

	useEffect(() => {
		let active = true;
		setRepairChecking(true);
		setRepairCheckError(null);
		void invoke<InstallationRepairStatus>(
			"check_installation_repair",
		)
			.then((status) => {
				if (active) setRepairStatus(status);
			})
			.catch((error: unknown) => {
				if (active) setRepairCheckError(String(error));
			})
			.finally(() => {
				if (active) setRepairChecking(false);
			});
		return () => {
			active = false;
		};
	}, [repairCheckAttempt]);

	const openRepairInstaller = async () => {
		if (!repairStatus?.assetUrl) return;
		setOpeningRepair(true);
		setRepairOpenError(null);
		try {
			await openUrl(repairStatus.assetUrl);
		} catch (error) {
			setRepairOpenError(
				`Could not open the installer download. Check your default browser, or open the GitHub releases page manually. (${String(error)})`,
			);
		} finally {
			setOpeningRepair(false);
		}
	};

	const openReleasesPage = async () => {
		setRepairOpenError(null);
		try {
			await openUrl(RELEASES_PAGE_URL);
		} catch (error) {
			setRepairOpenError(
				`Could not open the GitHub releases page. Visit ${RELEASES_PAGE_URL} in your browser. (${String(error)})`,
			);
		}
	};

	const severity = severityOf(report);
	const verified =
		manifest?.available === true && manifest.signatureValid;
	const canAct =
		verified &&
		installationAction !== null &&
		installationAction !== "blocked";
	const actionLabel =
		installationAction === "launch"
			? "LAUNCH"
			: installationAction === "update"
				? "UPDATE"
				: installationAction === "setup"
					? "SETUP"
					: "SETUP LOCKED";
	const scanned = report !== null && report.error === null;
	const progress =
		scanned && report.totalFiles > 0
			? Math.round((report.okCount / report.totalFiles) * 100)
			: 0;

	const summary = manifest?.error
		? "Manifest unavailable -> Mars Command cannot confirm which files you should be running"
		: !instanceRoot
			? "Choose the Minecraft game folder in Settings to compare local files against the signed manifest"
			: severity === "clean"
				? "All managed files match the approved manifest"
				: severity === "bad"
					? "Local files diverge from the approved manifest. Repair is not available in this build"
					: "Manifest verified -> Some files differ but none are critical";

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
				{scanned && report.personalMods.length > 0 ? (
					<div className="mt-2 text-[10px] leading-4 text-cyan-200">
						{report.personalMods.length} personal mod(s), unsigned and separate from signed verification. Manage in Settings.
						<ul className="max-h-24 overflow-y-auto pr-1">
							{report.personalMods.map((entry) => (
								<li key={entry.file.fileName} className={entry.status === "installed" ? "" : "text-amber-200"}>
									{entry.file.fileName}: {entry.status === "installed" ? "local checksum matches" : entry.status}
									{entry.message ? ` - ${entry.message}` : ""}
								</li>
							))}
						</ul>
					</div>
				) : null}

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
					role="status"
					className={`mb-2 text-center text-[10px] leading-4 ${
						installationMessage &&
						launcherOpened
							? "text-emerald-200/80"
							: installationMessage
								? "text-amber-200/80"
								: verified
									? "text-cyan-100/75"
									: "text-amber-200/80"
					}`}
				>
					{installationMessage ??
						(installationAction === "launch"
							? "Mars is current and verified. Launch Minecraft Launcher."
							: installationAction ===
								  "update"
								? "The installation or server manifest changed. Update Mars to continue."
								: installationAction ===
									  "setup"
									? "No Mars installation found. Set up an isolated game directory."
									: verified
										? "Pack integrity needs attention before launch."
										: "A trusted signed manifest is required before setup.")}
				</p>
				<button
					type="button"
					disabled={!canAct || syncing || personalModBusy}
					onClick={setupLauncherInstallation}
					title={
						!canAct
							? "A trusted signed manifest is required."
							: installationAction ===
								  "launch"
								? "Launch the already verified Mars installation."
								: "Create or update the isolated Mars game directory, verify the files, then open Minecraft Launcher."
					}
					className={`flex w-full items-center justify-center gap-2.5 rounded-xl border px-4 py-3.5 text-[13px] font-bold tracking-[0.12em] ${
						canAct && !syncing
							? "border-emerald-300/20 bg-emerald-400/10 text-emerald-200 hover:bg-emerald-400/15"
							: "border-white/10 bg-white/5 text-slate-500"
					}`}
				>
					{syncing ? (
						<LoaderCircle
							size={18}
							className="animate-spin"
						/>
					) : verified ? (
						<Download size={18} />
					) : (
						<Lock size={16} />
					)}
					{syncing
						? "SETTING UP MARS..."
						: actionLabel}
				</button>
				<div className="mt-2 border-t border-white/8 pt-2">
					{repairChecking ? (
						<p className="text-center text-[10px] text-slate-500">
							Checking for a
							compatible setup
							release...
						</p>
					) : repairCheckError ? (
						<div className="space-y-1.5 text-[10px] text-amber-200/80">
							<p role="status">
								Stable update check
								failed:{" "}
								{repairCheckError}
							</p>
							<div className="flex items-center gap-3">
								<button
									type="button"
									className="flex items-center gap-1 text-cyan-200 hover:text-cyan-100"
									onClick={() =>
										setRepairCheckAttempt(
											(attempt) =>
												attempt +
												1,
										)
									}
									title="Retry the stable release check"
								>
									<RefreshCw
										size={12}
									/>
									Retry check
								</button>
								<button
									type="button"
									className="flex items-center gap-1 text-cyan-200 hover:text-cyan-100"
									onClick={() =>
										void openReleasesPage()
									}
								>
									<ExternalLink
										size={12}
									/>
									Open releases page
								</button>
							</div>
						</div>
					) : repairStatus?.updateAvailable &&
					  repairStatus.assetUrl ? (
						<div className="rounded-lg border border-amber-300/20 bg-amber-300/5 p-2.5">
							<div className="flex items-center justify-between gap-2">
								<div className="min-w-0">
									<p className="text-[10px] font-semibold tracking-wide text-amber-100">
										STABLE CLIENT
										UPDATE AVAILABLE
									</p>
									<p className="mt-0.5 text-[10px] text-slate-300">
										Installed{" "}
										{
											repairStatus.installedVersion
										}{" "}
										· Stable{" "}
										{
											repairStatus.latestVersion
										}
										{" "}for{" "}
										{repairStatus.platform}
									</p>
								</div>
								<button
									type="button"
									disabled={
										openingRepair
									}
									onClick={() =>
										void openRepairInstaller()
									}
									className="flex shrink-0 items-center gap-1.5 rounded-md border border-amber-200/25 bg-amber-200/10 px-2.5 py-1.5 text-[10px] font-semibold text-amber-100 hover:bg-amber-200/15 disabled:opacity-50"
									title="Open the stable installer compatible with this platform"
								>
									{openingRepair ? (
										<LoaderCircle
											size={
												12
											}
											className="animate-spin"
										/>
									) : (
										<ExternalLink
											size={
												12
											}
										/>
									)}
									Open installer
								</button>
							</div>
							<p className="mt-1.5 text-[10px] leading-4 text-slate-400">
								Choose{" "}
								<strong>Open installer</strong>{" "}
								to start the download in your
								browser. When it finishes,
								close Mars Command, open the
								downloaded installer, and
								follow its prompts. Then
								relaunch Mars Command. The
								installer is never run
								automatically.
							</p>
						</div>
					) : (
						<div className="space-y-1.5 text-[10px] text-slate-500">
							<p>
								{repairStatus?.message ??
									`Setup build ${repairStatus?.installedVersion ?? ""} is current.`}
							</p>
							{repairStatus?.message ? (
								<div className="flex items-center gap-3">
									<button
										type="button"
										className="flex items-center gap-1 text-cyan-200 hover:text-cyan-100"
										onClick={() =>
											setRepairCheckAttempt(
												(attempt) =>
													attempt +
													1,
											)
										}
										title="Retry the stable release check"
									>
										<RefreshCw
											size={12}
										/>
										Retry check
									</button>
									<button
										type="button"
										className="flex items-center gap-1 text-cyan-200 hover:text-cyan-100"
										onClick={() =>
											void openReleasesPage()
										}
									>
										<ExternalLink
											size={12}
										/>
										Open releases page
									</button>
								</div>
							) : null}
						</div>
					)}
					{repairOpenError ? (
						<p
							role="alert"
							className="mt-1.5 text-[10px] leading-4 text-red-200"
						>
							{repairOpenError}
						</p>
					) : null}
				</div>
			</div>
		</Panel>
	);
}

export default DeploymentPanel;
