const PRIVACY_URL = "https://www.onorca.dev/docs/telemetry";
function isTelemetryConsentState(x) {
	if (!x || typeof x !== "object") return false;
	const e = x.effective;
	if (e === "enabled" || e === "pending_banner") return true;
	if (e === "disabled") {
		const r = x.reason;
		return r === "do_not_track" || r === "orca_disabled" || r === "ci" || r === "user_opt_out";
	}
	return false;
}
function track(name, props) {
	try {
		window.api?.telemetryTrack?.(name, props)?.catch((err) => {
			console.warn("[telemetry] IPC track failed", err);
		});
	} catch (err) {
		console.warn("[telemetry] IPC track threw synchronously", err);
	}
}
function setOptIn(optedIn) {
	try {
		return window.api?.telemetrySetOptIn?.(optedIn)?.catch((err) => {
			console.warn("[telemetry] IPC setOptIn failed", err);
		}) ?? Promise.resolve();
	} catch (err) {
		console.warn("[telemetry] IPC setOptIn threw synchronously", err);
		return Promise.resolve();
	}
}
async function getConsentState() {
	try {
		const result = await window.api?.telemetryGetConsentState?.();
		return isTelemetryConsentState(result) ? result : { effective: "pending_banner" };
	} catch (err) {
		console.warn("[telemetry] IPC getConsentState failed", err);
		return { effective: "pending_banner" };
	}
}
function acknowledgeBanner() {
	try {
		return window.api?.telemetryAcknowledgeBanner?.()?.catch((err) => {
			console.warn("[telemetry] IPC acknowledgeBanner failed", err);
		}) ?? Promise.resolve();
	} catch (err) {
		console.warn("[telemetry] IPC acknowledgeBanner threw synchronously", err);
		return Promise.resolve();
	}
}
export { track as a, setOptIn as i, acknowledgeBanner as n, getConsentState as r, PRIVACY_URL as t };
