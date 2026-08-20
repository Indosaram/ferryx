import "./react-Da2TLWQy.js";
import "./jsx-runtime-Cv_nyRjc.js";
import "./button-DszXJEV6.js";
import { t as useAppStore } from "./store-CgXrfmaH.js";
import "./plugin-manifest-Bs-50M_g.js";
import "./agent-status-3vUKbY6l.js";
import "./react-dom-Da8MQai-.js";
import "./dist-DgqligFk.js";
import "./agent-kind-Dfx6MnkP.js";
import "./telemetry-ZyUPyKMD.js";
import "./useMountedRef-1omUd-IV.js";
import "./feature-wall-setup-steps-D7ga1-7b.js";
import "./use-active-skill-discovery-runtime-target-CdctmJyj.js";
import "./useInstalledAgentSkills-BYdWqfUf.js";
import "./project-skill-runtime-BxhwlKnI.js";
import "./useActiveProjectSkillRuntime-CZKRPBhf.js";
import { t as useSetupGuideProgress } from "./use-setup-guide-progress-ClKjN7cj.js";
import "./use-integration-connection-status-B2txbF2j.js";
import "./feature-education-telemetry-DPRGAVBD.js";
import { n as useSetupGuideStepCompletionTelemetry } from "./use-setup-guide-telemetry-iL_4SQTa.js";
function SetupGuideTelemetryObserver() {
	const setupGuideVisible = useAppStore((s) => s.activeModal === "setup-guide");
	useSetupGuideStepCompletionTelemetry({
		progress: useSetupGuideProgress(true, false, false),
		setupGuideVisible
	});
	return null;
}
export { SetupGuideTelemetryObserver };
