export type StatusTone = "good" | "warning" | "danger" | "info" | "idle";

const TONES: Record<StatusTone, string> = {
	good: "bg-emerald-400 shadow-[0_0_12px_rgba(74,222,128,0.85)]",
	warning: "bg-amber-300 shadow-[0_0_12px_rgba(252,211,77,0.85)]",
	danger: "bg-red-400 shadow-[0_0_12px_rgba(248,113,113,0.85)]",
	info: "bg-cyan-300 shadow-[0_0_12px_rgba(103,232,249,0.85)]",
	idle: "bg-slate-500",
};

export function StatusDot({
	tone = "good",
	pulse = false,
}: {
	tone?: StatusTone;
	pulse?: boolean;
}) {
	return (
		<span
			className={`h-2.5 w-2.5 rounded-full ${TONES[tone]} ${
				pulse ? "animate-pulse" : ""
			}`}
		/>
	);
}

export default StatusDot;
