function dashboardBucketForDotState(state) {
	switch (state) {
		case "working": return "working";
		case "done": return "done";
		case "idle": return "idle";
		case "blocked":
		case "waiting": return "attention";
	}
}
export { dashboardBucketForDotState as t };
