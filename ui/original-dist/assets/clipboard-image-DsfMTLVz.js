import { Bm as ORCA_BROWSER_PARTITION } from "./store-CgXrfmaH.js";
const DEFAULT_LOCAL_ORCA_PROFILE_ID = "local-default";
const DEFAULT_LOCAL_ORCA_PROFILE_NAME = "Personal";
function createDefaultLocalOrcaProfile(now) {
	return {
		id: DEFAULT_LOCAL_ORCA_PROFILE_ID,
		name: DEFAULT_LOCAL_ORCA_PROFILE_NAME,
		avatar: {
			kind: "initials",
			initials: "P",
			color: "neutral"
		},
		kind: "local",
		createdAt: now,
		updatedAt: now,
		lastOpenedAt: now
	};
}
function profilePartitionHash(value) {
	let hash = 2166136261;
	for (let i = 0; i < value.length; i++) {
		hash ^= value.charCodeAt(i);
		hash = Math.imul(hash, 16777619);
	}
	return (hash >>> 0).toString(16).padStart(8, "0");
}
function getOrcaProfileBrowserPartitionSegment(profileId) {
	return `${profileId.replace(/[^A-Za-z0-9_-]/g, "_").slice(0, 48) || "profile"}-${profilePartitionHash(profileId)}`;
}
function getOrcaProfileBrowserDefaultPartition(profileId) {
	if (profileId === "local-default") return ORCA_BROWSER_PARTITION;
	return `persist:orca-profile-${getOrcaProfileBrowserPartitionSegment(profileId)}-browser-default`;
}
const CLIPBOARD_IMAGE_MAX_BASE64_CHARS = 24 * 1024 * 1024;
const CLIPBOARD_IMAGE_MAX_SOURCE_BYTES = Math.floor(CLIPBOARD_IMAGE_MAX_BASE64_CHARS / 4 * 3);
const CLIPBOARD_IMAGE_MAX_PIXELS = 32 * 1024 * 1024;
const CLIPBOARD_IMAGE_TOO_LARGE_ERROR = "Clipboard image is too large";
function assertClipboardImageByteLengthWithinLimit(byteLength) {
	if (!Number.isFinite(byteLength) || byteLength > CLIPBOARD_IMAGE_MAX_SOURCE_BYTES) throw new Error(CLIPBOARD_IMAGE_TOO_LARGE_ERROR);
}
function assertClipboardImageDimensionsWithinLimit({ height, width }) {
	const pixelCount = width * height;
	if (!Number.isFinite(pixelCount) || width <= 0 || height <= 0 || pixelCount > 33554432) throw new Error(CLIPBOARD_IMAGE_TOO_LARGE_ERROR);
}
export { assertClipboardImageByteLengthWithinLimit as a, createDefaultLocalOrcaProfile as c, CLIPBOARD_IMAGE_TOO_LARGE_ERROR as i, getOrcaProfileBrowserDefaultPartition as l, CLIPBOARD_IMAGE_MAX_PIXELS as n, assertClipboardImageDimensionsWithinLimit as o, CLIPBOARD_IMAGE_MAX_SOURCE_BYTES as r, DEFAULT_LOCAL_ORCA_PROFILE_ID as s, CLIPBOARD_IMAGE_MAX_BASE64_CHARS as t };
