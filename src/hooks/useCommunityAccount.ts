import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useRef, useState } from "react";
import type { CommunityUser, LoginPoll, LoginStart } from "../types/community";
import { ACCOUNT_CLEANUP_FAILURE, withSecondaryFailure } from "../lib/accountErrors";

export function useCommunityAccount() {
	const [user, setUser] = useState<CommunityUser | null>(null);
	const [pending, setPending] = useState(false);
	const [message, setMessage] = useState("");
	const generation = useRef(0);
	const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	const busy = useRef(false);
	const clearTimer = () => { clearTimeout(timer.current); timer.current = undefined; };

	const refresh = useCallback(async () => {
		const current = generation.current;
		const identity = await invoke<CommunityUser | null>("desktop_account");
		if (current === generation.current) setUser(identity);
		return identity;
	}, []);

	useEffect(() => {
		let active = true;
		void invoke<CommunityUser | null>("desktop_account")
			.then((identity) => { if (active) setUser(identity); })
			.catch(() => { if (active) setMessage("Desktop account state is unavailable."); });
		return () => {
			active = false;
			generation.current++;
			clearTimer();
			// Ignore late HTTP approval after the account UI is unmounted.
			void invoke("desktop_logout").catch(() => {
				console.error("Desktop account unmount cleanup failed; close the client to clear session memory.");
			});
		};
	}, []);

	const logout = useCallback(async () => {
		generation.current++;
		clearTimer();
		busy.current = false;
		setPending(false);
		setUser(null);
		try {
			await invoke("desktop_logout");
			setMessage("Desktop session cleared. Website login is independent.");
		} catch {
			setMessage("Could not clear backend account state. Close the client to end this session.");
		}
	}, []);

	const login = useCallback(async () => {
		if (busy.current) return;
		busy.current = true;
		clearTimer();
		const attempt = ++generation.current;
		setPending(true);
		setMessage("Starting website GitHub sign-in…");
		try {
			const start = await invoke<LoginStart>("desktop_login_start");
			if (attempt !== generation.current) return;
			const deadline = Date.now() + start.expiresIn * 1000;
			// Browser timers overflow above signed 32-bit milliseconds; Rust still enforces the API interval.
			const pollDelay = Math.min(start.pollInterval * 1000, 2_147_483_647);
			await openUrl(start.verificationUri);
			if (attempt !== generation.current) return;
			setMessage("Complete GitHub sign-in and explicitly approve this desktop on the website.");
			const poll = async () => {
				if (attempt !== generation.current) return;
				if (Date.now() >= deadline) {
					try {
						await invoke("desktop_logout");
						if (attempt !== generation.current) return;
						setMessage("Login expired. Start a new website sign-in.");
					} catch {
						if (attempt !== generation.current) return;
						setMessage("Login expired, but backend cleanup failed. Close the client before retrying.");
					}
					if (attempt === generation.current) {
						busy.current = false;
						setPending(false);
					}
					return;
				}
				try {
					const result = await invoke<LoginPoll>("desktop_login_poll");
					if (attempt !== generation.current) return;
					if (result.status === "pending") {
						timer.current = setTimeout(() => void poll(), pollDelay);
						return;
					}
					busy.current = false;
					setPending(false);
					if (result.status === "approved" && result.user) {
						setUser(result.user);
						setMessage(`Signed in as ${result.user.username}. Session lasts only while this client is open.`);
					} else {
						setMessage(result.status === "denied" ? "Website approval was denied. You can retry." : "Login expired. Please retry.");
					}
				} catch (error) {
					if (attempt !== generation.current) return;
					busy.current = false;
					setPending(false);
					setMessage(String(error));
				}
			};
			timer.current = setTimeout(() => void poll(), pollDelay);
		} catch (error) {
			if (attempt !== generation.current) return;
			const failureMessage = await withSecondaryFailure(
				`Could not start website login: ${String(error)}`,
				() => invoke("desktop_logout"),
				ACCOUNT_CLEANUP_FAILURE,
			);
			if (attempt !== generation.current) return;
			busy.current = false;
			setPending(false);
			setMessage(failureMessage);
		}
	}, []);

	const donate = useCallback(async () => {
		if (!user) {
			setMessage("Website sign-in is required before opening the configured Sponsors page.");
			await login();
			return;
		}
		try {
			await openUrl(await invoke<string>("desktop_sponsors_url"));
			setMessage("GitHub handles sponsorships. Desktop roles are not changed by this button.");
		} catch (error) {
			setMessage(String(error));
		}
	}, [login, user]);

	return { user, pending, message, login, logout, donate, refresh };
}

export type CommunityAccount = ReturnType<typeof useCommunityAccount>;
