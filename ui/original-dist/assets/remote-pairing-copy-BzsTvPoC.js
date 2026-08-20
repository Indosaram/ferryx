import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { a as parsePairingCode } from "./store-CgXrfmaH.js";
import { t as isTailnetIPv4Address } from "./tailnet-address-B7o4AlJU.js";
var LOOPBACK_HOSTS = new Set([
	"localhost",
	"localhost.localdomain",
	"localhost6",
	"localhost6.localdomain6",
	"ip6-localhost",
	"ip6-loopback",
	"127.0.0.1",
	"::1"
]);
function isPrivateIPv4Address(hostname) {
	const octets = hostname.split(".").map(Number);
	if (octets.length !== 4 || octets.some((octet) => !Number.isInteger(octet))) return false;
	return octets[0] === 10 || octets[0] === 172 && octets[1] >= 16 && octets[1] <= 31 || octets[0] === 192 && octets[1] === 168;
}
function isPrivateIPv6Address(hostname) {
	const firstHextet = Number.parseInt(hostname.split(":")[0] ?? "", 16);
	return Number.isInteger(firstHextet) && ((firstHextet & 65024) === 64512 || (firstHextet & 65472) === 65152);
}
function getEmbeddedIPv4Address(hostname) {
	const match = hostname.match(/^::(?:ffff:)?([0-9a-f]{1,4}):([0-9a-f]{1,4})$/i);
	if (!match) return null;
	const high = Number.parseInt(match[1], 16);
	const low = Number.parseInt(match[2], 16);
	return `${high >> 8}.${high & 255}.${low >> 8}.${low & 255}`;
}
function classifyRemotePairingHostname(hostname) {
	const normalized = hostname.toLowerCase().replace(/^\[|\]$/g, "").replace(/\.$/, "");
	const embeddedIPv4 = getEmbeddedIPv4Address(normalized);
	if (embeddedIPv4) return classifyRemotePairingHostname(embeddedIPv4);
	if (LOOPBACK_HOSTS.has(normalized) || normalized.endsWith(".localhost") || normalized.startsWith("127.")) return "loopback";
	if (isTailnetIPv4Address(normalized)) return "tailscale";
	if (isPrivateIPv4Address(normalized) || isPrivateIPv6Address(normalized)) return "lan";
	return normalized.includes(".") || normalized.includes(":") ? "public" : "custom";
}
function parseHostAccessLink(input) {
	const pairing = parsePairingCode(input);
	if (!pairing) return {
		ok: false,
		kind: "invalid-input",
		message: "Enter an Orca access link or bare pairing code."
	};
	if (pairing.scope === "mobile") return {
		ok: false,
		kind: "mobile-only",
		message: "This link grants mobile-only access. Generate a link for another Orca client."
	};
	let endpoint;
	try {
		endpoint = new URL(pairing.endpoint);
	} catch {
		return {
			ok: false,
			kind: "invalid-destination",
			message: "This access link contains an invalid destination."
		};
	}
	if (endpoint.protocol !== "ws:" && endpoint.protocol !== "wss:" || !endpoint.hostname || endpoint.hash !== "") return {
		ok: false,
		kind: "unsupported-destination",
		message: "This access link contains an unsupported destination."
	};
	const normalizedHostname = endpoint.hostname.toLowerCase().replace(/^\[|\]$/g, "");
	if (normalizedHostname === "0.0.0.0" || normalizedHostname === "::" || getEmbeddedIPv4Address(normalizedHostname) === "0.0.0.0" || endpoint.port === "0") return {
		ok: false,
		kind: "non-connectable-destination",
		message: "This access link contains a non-connectable destination."
	};
	return {
		ok: true,
		value: {
			pairing,
			displayEndpoint: endpoint.host,
			endpointKind: classifyRemotePairingHostname(endpoint.hostname)
		}
	};
}
function translateHostAccessLinkError(kind) {
	switch (kind) {
		case "invalid-input": return translate("auto.lib.remotePairingCopy.invalidInput", "Enter an Orca access link or bare pairing code.");
		case "mobile-only": return translate("auto.lib.remotePairingCopy.mobileOnly", "This link grants mobile-only access. Generate a link for another Orca client.");
		case "invalid-destination": return translate("auto.lib.remotePairingCopy.invalidDestination", "This access link contains an invalid destination.");
		case "unsupported-destination": return translate("auto.lib.remotePairingCopy.unsupportedDestination", "This access link contains an unsupported destination.");
		case "non-connectable-destination": return translate("auto.lib.remotePairingCopy.nonConnectableDestination", "This access link contains a non-connectable destination.");
	}
}
function translateRemotePairingEndpointKind(kind) {
	switch (kind) {
		case "loopback": return translate("auto.lib.remotePairingCopy.loopback", "Loopback");
		case "tailscale": return translate("auto.lib.remotePairingCopy.tailscale", "Tailscale address");
		case "lan": return translate("auto.lib.remotePairingCopy.lan", "Private LAN address");
		case "public": return translate("auto.lib.remotePairingCopy.public", "Public address");
		case "custom": return translate("auto.lib.remotePairingCopy.custom", "Custom hostname");
	}
}
function translateRemotePairingFailureDescription(kind, endpoint) {
	switch (kind) {
		case "host-identity-mismatch": return translate("auto.components.settings.RuntimeHostAccessForm.identityMismatchHelp", "Orca reached {{endpoint}}, but that host does not match this link. Generate a new link on the other host.", { endpoint: endpoint ?? "Orca" });
		case "access-link-invalid": return translate("auto.components.settings.RuntimeHostAccessForm.invalidLinkHelp", "Generate a new access link on the other host and try again.");
		case "protocol-incompatible": return translate("auto.components.settings.RuntimeHostAccessForm.incompatibleHelp", "Update Orca on this device and the other host, then try again.");
		case "connection-interrupted": return translate("auto.components.settings.RuntimeHostAccessForm.interruptedHelp", "The connection stopped during verification. Check the network or SSH tunnel and try again.");
		case "environment-save-failed": return translate("auto.components.settings.RuntimeHostAccessForm.saveFailedHelp", "The host was verified, but Orca could not save it. Check the name and local settings storage, then try again.");
		case "host-unreachable": return translate("auto.components.settings.RuntimeHostAccessForm.unavailableHelp", "Make sure Orca is running on the other host and that the network or SSH tunnel can reach {{endpoint}}.", { endpoint: endpoint ?? "Orca" });
	}
}
export { parseHostAccessLink as i, translateRemotePairingEndpointKind as n, translateRemotePairingFailureDescription as r, translateHostAccessLinkError as t };
