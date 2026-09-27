import type { ReactNode } from "react";

export function Panel({
	title,
	icon,
	actions,
	children,
	className = "",
	bodyClassName = "",
}: {
	title: string;
	icon: ReactNode;
	actions?: ReactNode;
	children: ReactNode;
	className?: string;
	bodyClassName?: string;
}) {
	return (
		<section
			className={[
				"flex min-h-0 flex-col rounded-lg border border-white/10 bg-slate-950/52 p-4",
				"shadow-[0_18px_50px_rgba(0,0,0,0.22)] backdrop-blur-2xl",
				className,
			].join(" ")}
		>
			<header className="mb-3 flex shrink-0 items-center gap-2 text-[11px] font-semibold tracking-[0.18em] text-slate-300 uppercase">
				<span className="text-cyan-200">{icon}</span>
				<span className="truncate">{title}</span>
				{actions ? (
					<span className="ml-auto">
						{actions}
					</span>
				) : null}
			</header>
			<div className={`min-h-0 flex-1 ${bodyClassName}`}>
				{children}
			</div>
		</section>
	);
}

export default Panel;
