import {
	MessageSquareText,
	RefreshCw,
	RotateCcw,
	Save,
	Settings2,
} from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { MARS_SERVER } from "../lib/mars";
import {
	isValidMissionControlHost,
	isValidMissionControlPort,
	type MissionControlTarget,
} from "../hooks/useMissionControlConfig";
import type { MinecraftServerStatus } from "../types/mars";
import { Panel } from "./Panel";
import { StatusDot } from "./StatusDot";

type MissionControlPanelProps = {
	target: MissionControlTarget;
	onApply: (target: MissionControlTarget) => void;
	storageError: string | null;
};

export function MissionControlPanel({
	target,
	onApply,
	storageError,
}: MissionControlPanelProps) {
	const [host, setHost] = useState(target.host);
	const [port, setPort] = useState(String(target.port));
	const [notice, setNotice] = useState("");

	useEffect(() => {
		setHost(target.host);
		setPort(String(target.port));
	}, [target.host, target.port]);

	const normalizedHost = host.trim();
	const portNumber = port.trim() === "" ? Number.NaN : Number(port);
	const hostValid = isValidMissionControlHost(normalizedHost);
	const portValid = isValidMissionControlPort(portNumber);
	const changed =
		normalizedHost !== target.host || portNumber !== target.port;

	function applyTarget(event: FormEvent<HTMLFormElement>) {
		event.preventDefault();
		if (!hostValid || !portValid) return;
		onApply({ host: normalizedHost, port: portNumber });
		setNotice("Status probe target applied.");
	}

	function resetTarget() {
		const defaultTarget = {
			host: MARS_SERVER.host,
			port: MARS_SERVER.port,
		};
		setHost(defaultTarget.host);
		setPort(String(defaultTarget.port));
		onApply(defaultTarget);
		setNotice("Default status probe target restored.");
	}

	return (
		<Panel title="Mission Control" icon={<Settings2 size={14} />}>
			<form className="space-y-2.5" onSubmit={applyTarget}>
				<label className="block space-y-1">
					<span className="text-[10px] text-slate-400">
						Status probe host
					</span>
					<input
						autoComplete="off"
						className="w-full rounded border border-white/12 bg-black/25 px-2 py-1.5 font-mono text-[11px] text-slate-100 shadow-none outline-none focus:border-cyan-300/60"
						maxLength={253}
						name="host"
						onChange={(event) => {
							setHost(
								event.target
									.value,
							);
							setNotice("");
						}}
						placeholder="play.example.net"
						spellCheck={false}
						value={host}
						aria-invalid={!hostValid}
					/>
					{!hostValid ? (
						<span className="block text-[10px] text-red-300">
							Enter a DNS name or IPv4
							address.
						</span>
					) : null}
				</label>

				<label className="block space-y-1">
					<span className="text-[10px] text-slate-400">
						Port
					</span>
					<input
						className="w-full rounded border border-white/12 bg-black/25 px-2 py-1.5 font-mono text-[11px] text-slate-100 shadow-none outline-none focus:border-cyan-300/60"
						inputMode="numeric"
						max={65535}
						min={1}
						name="port"
						onChange={(event) => {
							setPort(
								event.target
									.value,
							);
							setNotice("");
						}}
						type="number"
						value={port}
						aria-invalid={!portValid}
					/>
					{!portValid ? (
						<span className="block text-[10px] text-red-300">
							Use a whole number from
							1 to 65535.
						</span>
					) : null}
				</label>

				<p className="text-[10px] leading-4 text-slate-500">
					Changes dashboard status polling only.
					The Minecraft Launcher server entry is
					unchanged.
				</p>

				<div className="flex flex-wrap gap-2">
					<button
						className="inline-flex min-h-8 items-center gap-1.5 rounded border border-cyan-200/20 bg-cyan-300/8 px-2.5 text-[10px] font-medium text-cyan-100 shadow-none transition-colors hover:bg-cyan-300/15 disabled:cursor-not-allowed disabled:opacity-40"
						disabled={
							!hostValid ||
							!portValid ||
							!changed
						}
						type="submit"
					>
						<Save size={12} />
						Apply
					</button>
					<button
						className="inline-flex min-h-8 items-center gap-1.5 rounded border border-white/10 px-2.5 text-[10px] text-slate-300 shadow-none transition-colors hover:bg-white/5 disabled:cursor-not-allowed disabled:opacity-40"
						disabled={
							normalizedHost ===
								MARS_SERVER.host &&
							portNumber ===
								MARS_SERVER.port
						}
						onClick={resetTarget}
						type="button"
					>
						<RotateCcw size={12} />
						Defaults
					</button>
				</div>

				{notice ? (
					<p
						aria-live="polite"
						className="text-[10px] text-emerald-200"
					>
						{notice}
					</p>
				) : null}
				{storageError ? (
					<p
						aria-live="polite"
						className="text-[10px] text-amber-200"
					>
						{storageError}
					</p>
				) : null}
			</form>
		</Panel>
	);
}

type CrewChannelPanelProps = {
	status: MinecraftServerStatus | null;
	loading: boolean;
	refreshing: boolean;
	onRefresh: () => void;
};

export function CrewChannelPanel({
	status,
	loading,
	refreshing,
	onRefresh,
}: CrewChannelPanelProps) {
	const population = loading
		? "Checking the server's aggregate player count..."
		: !status
			? "Waiting for the first server status check."
			: !status.online
				? "The server status check did not report an online server."
				: status.playersOnline === null
					? "The server did not return an aggregate player count."
					: status.playersOnline === 0
						? "No players are currently reported online."
						: `${status.playersOnline} ${status.playersOnline === 1 ? "player is" : "players are"} reported online.`;

	return (
		<Panel
			title="Crew Channel"
			icon={<MessageSquareText size={14} />}
			actions={
				<span className="flex items-center gap-1.5 text-[10px] text-slate-500">
					<StatusDot
						tone="idle"
						pulse={loading || refreshing}
					/>
					NO CHAT API
				</span>
			}
		>
			<div className="space-y-3">
				<p className="text-[11px] leading-4 text-slate-300">
					The published Mars API does not expose a
					Crew roster or messaging endpoint.
				</p>
				<div className="space-y-1.5 border-t border-white/8 pt-3">
					<p className="text-[10px] text-slate-500">
						AGGREGATE PRESENCE
					</p>
					<p
						aria-live="polite"
						className={`text-[11px] leading-4 ${status && !status.online && !loading ? "text-amber-200" : "text-slate-400"}`}
					>
						{population}
					</p>
				</div>
				<p className="text-[10px] leading-4 text-slate-500">
					Only the Minecraft status protocol's
					total player count is available here;
					names and messages are not transmitted.
				</p>
				{status && !status.online && !loading ? (
					<button
						className="inline-flex min-h-8 items-center gap-1.5 rounded border border-white/10 px-2.5 text-[10px] text-slate-300 shadow-none transition-colors hover:bg-white/5"
						disabled={refreshing}
						onClick={onRefresh}
						type="button"
					>
						<RefreshCw size={12} />
						Retry status check
					</button>
				) : null}
			</div>
		</Panel>
	);
}
