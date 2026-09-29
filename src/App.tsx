import { Cloud, Cpu, Users, Volume2 } from "lucide-react";
import { useMemo, useState } from "react";
import "./App.css";
import "./index.css";
import { DeploymentPanel } from "./components/DeploymentPanel";
import { Panel } from "./components/Panel";
import { ServerStatusPanel } from "./components/ServerStatusPanel";
import { SettingsPage } from "./components/SettingsPage";
import { StatusDot } from "./components/StatusDot";
import { TitleBar } from "./components/TitleBar";
import { useMarsServerStatus } from "./hooks/useMarsServerStatus";
import { usePackIntegrity } from "./hooks/usePackIntegrity";
import { EM_DASH, formatClock, normalizeMotd } from "./lib/format";
import {
	CLIENT_BUILD,
	LOCAL_TRANSMISSIONS,
	MARS_SERVER,
	STATUS_REFRESH_INTERVAL_MS,
} from "./lib/mars";
import type { Transmission } from "./types/mars";

const TRANSMISSION_DOT: Record<Transmission["tone"], string> = {
	alert: "bg-red-300",
	notice: "bg-amber-300",
	server: "bg-cyan-300",
};

function TelemetryRow({
	label,
	value,
	tone = "text-slate-100",
}: {
	label: string;
	value: string;
	tone?: string;
}) {
	return (
		<div className="flex items-baseline justify-between gap-3">
			<span className="shrink-0 text-[10px] tracking-[0.12em] text-slate-500">
				{label}
			</span>
			<span
				className={`truncate font-mono text-[11px] ${tone}`}
			>
				{value}
			</span>
		</div>
	);
}

function App() {
	const { status, loading, refreshing, refresh, lastSuccessfulCheck } =
		useMarsServerStatus({
			host: MARS_SERVER.host,
			port: MARS_SERVER.port,
			refreshIntervalMs: STATUS_REFRESH_INTERVAL_MS,
		});

	const pack = usePackIntegrity();
	const [settingsOpen, setSettingsOpen] = useState(false);

	const transmissions = useMemo<Transmission[]>(() => {
		const motd = normalizeMotd(status?.motd);
		if (!motd) return LOCAL_TRANSMISSIONS;
		return [
			{
				id: "server-motd",
				title: "Server transmission",
				body: motd,
				tone: "server",
			},
			...LOCAL_TRANSMISSIONS,
		];
	}, [status?.motd]);

	const pendingFiles =
		pack.report && pack.report.error === null
			? pack.report.missingCount + pack.report.corruptCount
			: null;

	return (
		<main className="h-screen overflow-hidden bg-[radial-gradient(circle_at_18%_0%,rgba(167,41,41,0.25),transparent_35%),radial-gradient(circle_at_88%_92%,rgba(30,112,133,0.17),transparent_36%),rgba(5,8,12,0.56)]">
			<div className="mx-auto flex h-full max-w-[1400px] flex-col overflow-hidden border border-white/12 bg-slate-950/30 shadow-2xl shadow-black/40 backdrop-blur-xl">
				<TitleBar
					settingsOpen={settingsOpen}
					onToggleSettings={() =>
						setSettingsOpen((open) => !open)
					}
				/>

				{settingsOpen ? (
					<SettingsPage
						pack={pack}
						refreshingTelemetry={refreshing}
						refreshTelemetry={refresh}
					/>
				) : (
					<div className="grid min-h-0 flex-1 grid-cols-12 gap-3 p-3">
						{/* Left rail */}
						<aside className="col-span-3 flex min-h-0 flex-col gap-3">
							<ServerStatusPanel
								status={status}
								loading={
									loading
								}
								refreshing={
									refreshing
								}
								host={
									MARS_SERVER.host
								}
							/>

							<Panel
								title="Mission Control"
								icon={
									<Volume2
										size={
											14
										}
									/>
								}
							>
								<div className="space-y-2.5">
									<div className="flex items-center justify-between">
										<span className="text-[11px] text-slate-300">
											Voice
											relay
										</span>
										<span className="flex items-center gap-1.5 text-[10px] text-slate-500">
											<StatusDot tone="idle" />
											NOT
											CONFIGURED
										</span>
									</div>

									<div className="rounded-lg border border-white/8 bg-black/15 px-2.5 py-2 font-mono text-[11px] text-slate-300">
										{
											MARS_SERVER.voiceEndpoint
										}
									</div>

									<p className="text-[11px] leading-4 text-slate-400">
										The
										moon
										has
										been
										informed
										of
										your
										intended
										arrival.
									</p>
								</div>
							</Panel>

							<div className="flex-1" />
						</aside>

						{/* Centre column */}
						<section className="col-span-6 flex min-h-0 flex-col gap-3">
							<DeploymentPanel
								{...pack}
							/>

							{/* Absorbs vertical slack so no panel stretches into a void. */}
							<Panel
								title="Latest Transmission"
								icon={
									<Cloud
										size={
											14
										}
									/>
								}
								className="flex-1"
								bodyClassName="overflow-y-auto pr-1"
							>
								<div className="space-y-2.5">
									{transmissions.map(
										(
											entry,
										) => (
											<div
												className="flex gap-2.5"
												key={
													entry.id
												}
											>
												<span
													className={`mt-1.5 h-1.5 w-1.5 shrink-0 rounded-lg ${TRANSMISSION_DOT[entry.tone]}`}
												/>
												<div className="min-w-0">
													<p className="text-[12px] font-medium text-slate-100">
														{
															entry.title
														}
													</p>
													<p className="mt-0.5 text-[11px] leading-4 text-slate-400">
														{
															entry.body
														}
													</p>
												</div>
											</div>
										),
									)}
								</div>
							</Panel>
						</section>

						{/* Right rail */}
						<aside className="col-span-3 flex min-h-0 flex-col gap-3">
							<Panel
								title="Client Telemetry"
								icon={
									<Cpu
										size={
											14
										}
									/>
								}
							>
								<div className="space-y-2">
									<TelemetryRow
										label="GAME"
										value={
											pack
												.manifest
												?.minecraftVersion ??
											MARS_SERVER.minecraftVersion
										}
									/>
									<TelemetryRow
										label="LOADER"
										value={`${MARS_SERVER.loader.toUpperCase()}${
											pack
												.manifest
												?.loaderVersion
												? ` ${pack.manifest.loaderVersion}`
												: ""
										}`}
									/>
									<TelemetryRow
										label="SERVER BUILD"
										value={
											status?.versionName ??
											EM_DASH
										}
									/>
									<TelemetryRow
										label="LAST CONTACT"
										value={formatClock(
											lastSuccessfulCheck,
										)}
									/>
									<TelemetryRow
										label="FILES"
										value={
											pendingFiles ===
											null
												? EM_DASH
												: `${pendingFiles} PENDING`
										}
										tone={
											pendingFiles ===
											null
												? "text-slate-500"
												: pendingFiles >
													  0
													? "text-red-300"
													: "text-emerald-200"
										}
									/>
								</div>
							</Panel>

							<Panel
								title="Crew Channel"
								icon={
									<Users
										size={
											14
										}
									/>
								}
							>
								<p className="text-[11px] leading-4 text-slate-400">
									Mars
									Command
									does not
									publish
									a
									personnel
									roster.
									Only
									aggregate
									counts
									are
									transmitted.
								</p>
							</Panel>

							<div className="flex-1" />
						</aside>
					</div>
				)}

				<footer className="flex h-8 shrink-0 items-center justify-between border-t border-white/8 px-4 text-[9px] tracking-[0.16em] text-slate-500">
					<span>
						MARS COMMAND // BUILD{" "}
						{CLIENT_BUILD}
					</span>
					<span>
						UPDATED{" "}
						{formatClock(status?.checkedAt)}{" "}
						// MOON: INFORMED
					</span>
				</footer>
			</div>
		</main>
	);
}

export default App;
