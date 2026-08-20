import { t as useAppStore } from "./store-CgXrfmaH.js";
function requestContextualTourWhenReady(args) {
	const maxAttempts = args.maxAttempts ?? 20;
	const retryDelayMs = args.retryDelayMs ?? 100;
	let attempts = 0;
	let timeoutId = null;
	let cancelled = false;
	const attempt = () => {
		if (cancelled) return;
		if (args.shouldContinue && !args.shouldContinue()) {
			cancelled = true;
			return;
		}
		attempts += 1;
		const before = useAppStore.getState();
		if (before.activeContextualTourId && before.activeContextualTourId !== args.id) {
			if (args.waitForActiveTourToClear && attempts < maxAttempts) timeoutId = setTimeout(attempt, retryDelayMs);
			return;
		}
		before.requestContextualTour(args.id, args.source, args.wasFeaturePreviouslyInteracted, { force: true });
		if (useAppStore.getState().activeContextualTourId === args.id || attempts >= maxAttempts) return;
		timeoutId = setTimeout(attempt, retryDelayMs);
	};
	timeoutId = setTimeout(attempt, 0);
	return () => {
		cancelled = true;
		if (timeoutId !== null) clearTimeout(timeoutId);
	};
}
export { requestContextualTourWhenReady as t };
