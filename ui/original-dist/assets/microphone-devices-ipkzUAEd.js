const SYSTEM_DEFAULT_MICROPHONE_SELECT_VALUE = "system-default";
var AGGREGATE_DEVICE_IDS = new Set(["default", "communications"]);
var BASE_AUDIO_CONSTRAINTS = {
	channelCount: 1,
	echoCancellation: true,
	noiseSuppression: true,
	autoGainControl: true
};
function normalizeMicrophoneDeviceId(deviceId) {
	if (typeof deviceId !== "string") return null;
	const trimmed = deviceId.trim();
	if (trimmed.length === 0) return null;
	return AGGREGATE_DEVICE_IDS.has(trimmed) ? null : trimmed;
}
function buildAudioCaptureConstraints(deviceId) {
	const preferredDeviceId = normalizeMicrophoneDeviceId(deviceId);
	if (!preferredDeviceId) return { audio: { ...BASE_AUDIO_CONSTRAINTS } };
	return { audio: {
		...BASE_AUDIO_CONSTRAINTS,
		deviceId: { exact: preferredDeviceId }
	} };
}
function isMicrophoneDeviceConstraintError(error) {
	if (!error || typeof error !== "object") return false;
	const name = "name" in error ? String(error.name) : "";
	return name === "OverconstrainedError" || name === "NotFoundError";
}
function listVoiceMicrophoneDevices(devices) {
	return devices.filter((device) => device.kind === "audioinput" && normalizeMicrophoneDeviceId(device.deviceId) !== null).map((device, index) => ({
		deviceId: device.deviceId.trim(),
		label: device.label.trim() || `Microphone ${index + 1}`
	}));
}
function findSoleDeviceByLabel(devices, label) {
	const wanted = label?.trim().toLowerCase();
	if (!wanted) return null;
	const matches = devices.filter((device) => device.label.trim().toLowerCase() === wanted);
	return matches.length === 1 ? matches[0] ?? null : null;
}
function resolveMicrophoneDevice(args) {
	const preferredDeviceId = normalizeMicrophoneDeviceId(args.preferredDeviceId);
	if (!preferredDeviceId) return {
		deviceId: null,
		kind: "system-default"
	};
	if (!args.devices || args.devices.length === 0) return {
		deviceId: preferredDeviceId,
		kind: "unknown"
	};
	if (args.devices.some((device) => device.deviceId === preferredDeviceId)) return {
		deviceId: preferredDeviceId,
		kind: "exact"
	};
	const relabeled = findSoleDeviceByLabel(args.devices, args.preferredDeviceLabel);
	if (relabeled) return {
		deviceId: relabeled.deviceId,
		kind: "relabeled"
	};
	return {
		deviceId: null,
		kind: "missing"
	};
}
async function enumerateMicrophonesOrNull(enumerateDevices) {
	if (!enumerateDevices) return null;
	try {
		return listVoiceMicrophoneDevices(await enumerateDevices());
	} catch {
		return null;
	}
}
async function openMicrophoneCaptureStream(args) {
	const preferredDeviceId = normalizeMicrophoneDeviceId(args.preferredDeviceId);
	if (!preferredDeviceId) return {
		stream: await args.getUserMedia(buildAudioCaptureConstraints(null)),
		fellBackToDefaultMicrophone: false,
		usedDeviceId: null
	};
	const resolution = resolveMicrophoneDevice({
		devices: await enumerateMicrophonesOrNull(args.enumerateDevices),
		preferredDeviceId,
		preferredDeviceLabel: args.preferredDeviceLabel
	});
	if (resolution.kind === "missing") return {
		stream: await args.getUserMedia(buildAudioCaptureConstraints(null)),
		fellBackToDefaultMicrophone: true,
		usedDeviceId: null
	};
	const targetDeviceId = resolution.deviceId ?? preferredDeviceId;
	try {
		return {
			stream: await args.getUserMedia(buildAudioCaptureConstraints(targetDeviceId)),
			fellBackToDefaultMicrophone: false,
			usedDeviceId: targetDeviceId
		};
	} catch (error) {
		if (!isMicrophoneDeviceConstraintError(error)) throw error;
		return {
			stream: await args.getUserMedia(buildAudioCaptureConstraints(null)),
			fellBackToDefaultMicrophone: true,
			usedDeviceId: null
		};
	}
}
function microphoneDeviceIdFromSelectValue(value) {
	if (value === "system-default") return null;
	return normalizeMicrophoneDeviceId(value);
}
function buildVoiceMicrophoneSelectOptions(args) {
	const options = [{
		value: SYSTEM_DEFAULT_MICROPHONE_SELECT_VALUE,
		label: args.systemDefaultLabel
	}, ...args.devices.map((device) => ({
		value: device.deviceId,
		label: device.label
	}))];
	const resolution = resolveMicrophoneDevice({
		devices: args.devicesKnown ? args.devices : null,
		preferredDeviceId: args.preferredDeviceId,
		preferredDeviceLabel: args.preferredDeviceLabel
	});
	if (resolution.kind === "system-default") return {
		options,
		selectedValue: SYSTEM_DEFAULT_MICROPHONE_SELECT_VALUE
	};
	if (resolution.kind === "exact" || resolution.kind === "relabeled") return {
		options,
		selectedValue: resolution.deviceId ?? "system-default"
	};
	const preferredDeviceId = normalizeMicrophoneDeviceId(args.preferredDeviceId);
	if (!preferredDeviceId) return {
		options,
		selectedValue: SYSTEM_DEFAULT_MICROPHONE_SELECT_VALUE
	};
	const cachedLabel = args.preferredDeviceLabel?.trim();
	const baseLabel = cachedLabel && cachedLabel.length > 0 ? cachedLabel : preferredDeviceId;
	options.push(resolution.kind === "missing" ? {
		value: preferredDeviceId,
		label: `${baseLabel} (${args.unavailableSuffix})`,
		unavailable: true
	} : {
		value: preferredDeviceId,
		label: baseLabel
	});
	return {
		options,
		selectedValue: preferredDeviceId
	};
}
export { openMicrophoneCaptureStream as i, listVoiceMicrophoneDevices as n, microphoneDeviceIdFromSelectValue as r, buildVoiceMicrophoneSelectOptions as t };
