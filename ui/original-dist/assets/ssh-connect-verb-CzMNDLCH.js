import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
function sshConnectVerb(status) {
	switch (status) {
		case "auth-failed": return translate("auto.ssh.sshConnectVerb.reconnect", "Reconnect");
		case "error":
		case "reconnection-failed": return translate("auto.ssh.sshConnectVerb.retry", "Retry");
		case null:
		case void 0:
		case "connected":
		case "connecting":
		case "deploying-relay":
		case "disconnected":
		case "reconnecting": return translate("auto.ssh.sshConnectVerb.connect", "Connect");
	}
}
function sshConnectingLabel() {
	return translate("auto.ssh.sshConnectVerb.connecting", "Connecting…");
}
export { sshConnectingLabel as n, sshConnectVerb as t };
