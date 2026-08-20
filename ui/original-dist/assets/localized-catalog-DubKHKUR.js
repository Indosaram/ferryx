import { r as i18n } from "./jsx-runtime-Cv_nyRjc.js";
function createLocalizedCatalog(builder) {
	let cachedLocale;
	let cachedValue;
	return () => {
		if (cachedLocale !== i18n.language || cachedValue === void 0) {
			cachedLocale = i18n.language;
			cachedValue = builder();
		}
		return cachedValue;
	};
}
export { createLocalizedCatalog as t };
