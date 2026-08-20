import { B as number, F as boolean, H as record, I as discriminatedUnion, N as _null, P as array, R as lazy, U as string, V as object, W as union, z as literal } from "./plugin-manifest-Bs-50M_g.js";
import { i as MIN_SSH_RELAY_GRACE_PERIOD_SECONDS, r as MAX_SSH_RELAY_GRACE_PERIOD_SECONDS } from "./ssh-types-Caw2Ltsn.js";
var JsonValueSchema = lazy(() => union([
	string(),
	number(),
	boolean(),
	_null(),
	array(JsonValueSchema),
	record(string(), JsonValueSchema)
]));
var SavedPortForwardSchema = object({
	localPort: number().int().min(1).max(65535),
	remoteHost: string().min(1),
	remotePort: number().int().min(1).max(65535),
	label: string().min(1).optional()
}).strict();
const EphemeralVmRecipeSshTargetSchema = object({
	label: string().min(1),
	configHost: string().min(1).optional(),
	host: string().min(1),
	port: number().int().min(1).max(65535),
	username: string(),
	identityFile: string().min(1).optional(),
	identityAgent: string().min(1).optional(),
	identitiesOnly: boolean().optional(),
	proxyCommand: string().min(1).optional(),
	jumpHost: string().min(1).optional(),
	relayGracePeriodSeconds: number().int().refine((value) => value === 0 || value >= 60 && value <= 604800, `Relay grace period must be 0 or between 60 and ${MAX_SSH_RELAY_GRACE_PERIOD_SECONDS} seconds.`).optional(),
	portForwards: array(SavedPortForwardSchema).optional()
}).strict();
const EphemeralVmRecipeConnectionSchema = discriminatedUnion("type", [object({
	type: literal("orca-server"),
	pairingCode: string().min(1),
	projectRoot: string().min(1)
}).strict(), object({
	type: literal("ssh"),
	target: EphemeralVmRecipeSshTargetSchema,
	projectRoot: string().min(1)
}).strict()]);
union([object({
	schemaVersion: literal(1),
	pairingCode: string().min(1),
	projectRoot: string().min(1),
	userData: record(string(), JsonValueSchema).optional()
}).strict(), object({
	schemaVersion: literal(1),
	connection: EphemeralVmRecipeConnectionSchema,
	userData: record(string(), JsonValueSchema).optional()
}).strict()]);
function getEphemeralVmRecipeResultConnection(result) {
	if ("connection" in result) return result.connection;
	return {
		type: "orca-server",
		pairingCode: result.pairingCode,
		projectRoot: result.projectRoot
	};
}
function getEphemeralVmRecipeResultProjectRoot(result) {
	return getEphemeralVmRecipeResultConnection(result).projectRoot;
}
export { getEphemeralVmRecipeResultProjectRoot as t };
