var CONNECTING_BY_STATUS = {
	disconnected: false,
	connecting: true,
	"auth-failed": false,
	"deploying-relay": true,
	connected: false,
	reconnecting: true,
	"reconnection-failed": false,
	error: false
};
var CAN_CONNECT_BY_STATUS = {
	disconnected: true,
	connecting: false,
	"auth-failed": true,
	"deploying-relay": false,
	connected: false,
	reconnecting: false,
	"reconnection-failed": true,
	error: true
};
function isConnectingSshStatus(status) {
	return status ? CONNECTING_BY_STATUS[status] : false;
}
function canConnectSshStatus(status) {
	return status ? CAN_CONNECT_BY_STATUS[status] : false;
}
export { isConnectingSshStatus as n, canConnectSshStatus as t };
