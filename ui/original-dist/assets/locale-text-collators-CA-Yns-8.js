var baseSensitivityCollator;
var numericCollator;
function compareBaseSensitivityLocaleText(a, b) {
	baseSensitivityCollator ?? (baseSensitivityCollator = new Intl.Collator(void 0, { sensitivity: "base" }));
	return baseSensitivityCollator.compare(a, b);
}
function compareNumericLocaleText(a, b) {
	numericCollator ?? (numericCollator = new Intl.Collator(void 0, { numeric: true }));
	return numericCollator.compare(a, b);
}
export { compareNumericLocaleText as n, compareBaseSensitivityLocaleText as t };
