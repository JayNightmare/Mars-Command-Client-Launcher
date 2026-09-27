import { Activity } from "lucide-react";
import { Panel } from "./Panel";
import { StatusDot, type StatusTone } from "./StatusDot";
import {
	EM_DASH,
	formatClock,
	formatLatency,
	formatNumber,
} from "../lib/format";
import { describePopulation } from "../lib/mars";
import type { ConnectionPhase, MinecraftServerStatus } from "../types/mars";

type Props = {
	status: MinecraftServerStatus | null;
	loading: boolean;
	refreshing: boolean;
	host: string;
};

const PHASE: Record<
	ConnectionPhase,
	{ label: string; tone: StatusTone; text: string }
> = {
	checking: {
		label: "CHECKING",
		tone: "warning",
		text: "text-amber-200",
	},
	reconnecting: {
		label: "RECONNECTING",
		tone: "warning",
		text: "text-amber-200",
	},
	online: { label: "ONLINE", tone: "good", text: "text-emerald-200" },
	offline: { label: "OFFLINE", tone: "danger", text: "text-red-300" },
};

function derivePhase(
	status: MinecraftServerStatus | null,
	loading: boolean,
	refreshing: boolean,
): ConnectionPhase {
	if (loading || !status) return "checking";
	// Routine 15s polls must not make a healthy panel flicker amber.
	if (refreshing && !status.online) return "reconnecting";
	return status.online ? "online" : "offline";
}

function Row({
	label,
	value,
	tone = "text-slate-200",
}: {
	label: string;
	value: string;
	tone?: string;
}) {
	return (
		<div className="flex items-baseline justify-between gap-3">
			<span className="text-[11px] text-slate-500">
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

export function ServerStatusPanel({
	status,
	loading,
	refreshing,
	host,
}: Props) {
	const phase = derivePhase(status, loading, refreshing);
	const presentation = PHASE[phase];
	const pending = phase === "checking" || phase === "reconnecting";
	const online = status?.online === true;

	return (
		<Panel
			title="Server Status"
			icon={<Activity size={14} />}
			actions={
				<span
					className={`flex items-center gap-1.5 text-[10px] font-semibold ${presentation.text}`}
				>
					<StatusDot
						tone={presentation.tone}
						pulse={pending}
					/>
					{presentation.label}
				</span>
			}
		>
			<div className="space-y-3">
				<div className="flex items-baseline gap-2">
					<span className="font-mono text-3xl leading-none font-bold text-slate-100">
						{online
							? formatNumber(
									status.playersOnline,
								)
							: EM_DASH}
					</span>
					<span className="font-mono text-sm text-slate-500">
						/{" "}
						{online
							? formatNumber(
									status.playersMax,
								)
							: EM_DASH}
					</span>
				</div>
				<p className="text-[10px] tracking-[0.14em] text-slate-500 uppercase">
					Personnel detected
				</p>

				<p className="text-[11px] leading-4 text-slate-400">
					{describePopulation(status, loading)}
				</p>

				<div className="space-y-1.5 border-t border-white/8 pt-3">
					<Row
						label="Endpoint"
						value={host}
						tone="text-cyan-200"
					/>
					<Row
						label="Latency"
						value={
							online
								? formatLatency(
										status.latencyMs,
									)
								: EM_DASH
						}
						tone={
							online
								? "text-emerald-200"
								: "text-slate-500"
						}
					/>
					<Row
						label="Last check"
						value={formatClock(
							status?.checkedAt,
						)}
						tone="text-slate-400"
					/>
				</div>

				{status && !status.online && !loading ? (
					<p className="rounded-lg border border-red-300/15 bg-red-500/8 px-2.5 py-1.5 text-[10px] leading-4 text-red-200/80">
						Unable to contact server.
					</p>
				) : null}
			</div>
		</Panel>
	);
}

export default ServerStatusPanel;
