function isWindowVisible() {
	return typeof document === "undefined" || document.visibilityState === void 0 || document.visibilityState === "visible";
}
function installWindowVisibilityInterval(args) {
	const setIntervalFn = args.setIntervalFn ?? ((callback, intervalMs) => setInterval(callback, intervalMs));
	const clearIntervalFn = args.clearIntervalFn ?? ((handle) => clearInterval(handle));
	let intervalId = null;
	const stop = () => {
		if (!intervalId) return;
		clearIntervalFn(intervalId);
		intervalId = null;
	};
	const start = () => {
		if (intervalId || !isWindowVisible()) return;
		(args.runOnVisible ?? args.run)();
		intervalId = setIntervalFn(args.run, args.intervalMs);
	};
	const reconcile = () => {
		if (isWindowVisible()) start();
		else stop();
	};
	start();
	if (typeof document !== "undefined" && typeof document.addEventListener === "function") document.addEventListener("visibilitychange", reconcile);
	return () => {
		stop();
		if (typeof document !== "undefined" && typeof document.removeEventListener === "function") document.removeEventListener("visibilitychange", reconcile);
	};
}
export { isWindowVisible as n, installWindowVisibilityInterval as t };
