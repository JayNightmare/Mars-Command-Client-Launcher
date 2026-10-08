import { Heart, UserRound } from "lucide-react";
import type { CommunityAccount } from "../hooks/useCommunityAccount";
import { Panel } from "./Panel";

export function AccountPanel({ account }: { account: CommunityAccount }) {
	return (
		<Panel title="Website account" icon={<UserRound size={14} />}>
			<div className="space-y-3 text-xs text-slate-300">
				<p>{account.user ? `GitHub: ${account.user.username}` : "Sign in with GitHub on the configured Mars website."}</p>
				<p className="text-slate-500">Desktop login is session-only. Website sessions and signed-pack authentication are separate.</p>
				<div className="flex flex-wrap gap-2">
					{!account.user && !account.pending ? <button type="button" onClick={() => void account.login()} className="rounded-lg bg-cyan-300/15 px-3 py-2 text-cyan-200">Sign in on website</button> : null}
					{account.user || account.pending ? <button type="button" onClick={() => void account.logout()} className="rounded-lg border border-white/10 px-3 py-2">{account.pending ? "Cancel login" : "Sign out desktop"}</button> : null}
					<button type="button" disabled={account.pending} onClick={() => void account.donate()} className="flex items-center gap-2 rounded-lg border border-white/10 px-3 py-2 disabled:opacity-40"><Heart size={14} /> Donate via GitHub</button>
				</div>
				{account.message ? <p role="status" aria-live="polite" className="break-words text-amber-200">{account.message}</p> : null}
			</div>
		</Panel>
	);
}
