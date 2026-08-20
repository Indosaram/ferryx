function isTailnetIPv4Address(address) {
	const parts = address.split(".");
	if (parts.length !== 4) return false;
	const octets = parts.map((part) => {
		if (!/^\d+$/.test(part)) return NaN;
		return Number(part);
	});
	if (octets.some((octet) => !Number.isInteger(octet) || octet < 0 || octet > 255)) return false;
	return octets[0] === 100 && octets[1] >= 64 && octets[1] <= 127;
}
export { isTailnetIPv4Address as t };
