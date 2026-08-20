function addViewportSizeChangeListener(onChange, target = window) {
	let lastWidth = target.innerWidth;
	let lastHeight = target.innerHeight;
	const handleResize = () => {
		const { innerWidth, innerHeight } = target;
		if (innerWidth === lastWidth && innerHeight === lastHeight) return;
		lastWidth = innerWidth;
		lastHeight = innerHeight;
		onChange();
	};
	target.addEventListener("resize", handleResize);
	return () => target.removeEventListener("resize", handleResize);
}
export { addViewportSizeChangeListener as t };
