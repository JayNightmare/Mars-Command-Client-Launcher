import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Radio, Settings2, Square, X } from "lucide-react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { useCallback } from "react";

export function TitleBar({
	settingsOpen,
	onToggleSettings,
}: {
	settingsOpen: boolean;
	onToggleSettings: () => void;
}) {
	// `data-tauri-drag-region` only matches the exact event target, so child
	// nodes would swallow the drag. Start it explicitly instead.
	const startDrag = useCallback(
		(event: ReactPointerEvent<HTMLElement>) => {
			if (event.button !== 0) return;
			if ((event.target as HTMLElement).closest("button"))
				return;
			void getCurrentWindow().startDragging();
		},
		[],
	);

	const toggleMaximize = useCallback(() => {
		void getCurrentWindow().toggleMaximize();
	}, []);

	return (
		<header
			onPointerDown={startDrag}
			onDoubleClick={toggleMaximize}
			className="flex h-12 shrink-0 select-none items-center justify-between border-b border-white/10 pl-4"
		>
			<div className="flex items-center gap-3">
				<div className="grid h-7 w-7 place-items-center rounded-lg border border-red-300/20 bg-red-500/15 text-red-200">
					<Radio size={15} />
				</div>

				<div className="leading-tight">
					<p className="text-[12px] font-bold tracking-[0.22em] text-slate-100">
						MARS COMMAND
					</p>
					<p className="text-[9px] tracking-[0.2em] text-slate-500">
						CLIENT TRANSPORT // LZ-01
					</p>
				</div>
			</div>

			<div className="flex h-full items-center gap-1 pt-2 mb-2 pr-1">
				<button
					type="button"
					aria-label={
						settingsOpen
							? "Close settings"
							: "Open settings"
					}
					aria-pressed={settingsOpen}
					title={
						settingsOpen
							? "Close settings"
							: "Settings"
					}
					onClick={onToggleSettings}
					className={`grid h-full w-10 justify-center place-items-center rounded-lg transition hover:bg-white/8 ${settingsOpen ? "text-cyan-200" : "text-slate-400 hover:text-white"}`}
				>
					<Settings2 size={15} />
				</button>
				<button
					type="button"
					aria-label="Minimise"
					onClick={() =>
						void getCurrentWindow().minimize()
					}
					className="grid h-full w-12 place-items-center justify-center text-slate-400 transition hover:bg-white/8 hover:text-white"
				>
					<Minus size={15} />
				</button>
				<button
					type="button"
					aria-label="Maximise"
					onClick={toggleMaximize}
					className="grid h-full w-12 place-items-center justify-center text-slate-400 transition hover:bg-white/8 hover:text-white"
				>
					<Square size={12} />
				</button>
				<button
					type="button"
					aria-label="Close"
					onClick={() =>
						void getCurrentWindow().close()
					}
					className="grid h-full w-12 place-items-center justify-center rounded-tr-[22px] text-slate-400 transition hover:bg-red-500/80 hover:text-white"
				>
					<X size={15} />
				</button>
			</div>
		</header>
	);
}

export default TitleBar;
