export const ACCOUNT_CLEANUP_FAILURE =
	"Backend session cleanup failed; credentials may remain in memory. Close the client before retrying.";
export const ACCOUNT_REFRESH_FAILURE =
	"Desktop account refresh failed; session status could not be confirmed. Sign out or close the client before further authenticated actions.";

export async function withSecondaryFailure(
	primaryMessage: string,
	action: () => Promise<unknown>,
	secondaryMessage: string,
): Promise<string> {
	try {
		await action();
		return primaryMessage;
	} catch {
		// The secondary exception may contain sensitive transport details.
		return `${primaryMessage} ${secondaryMessage}`;
	}
}
