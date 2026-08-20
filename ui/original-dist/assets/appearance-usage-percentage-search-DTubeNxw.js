import { a as translate } from "./jsx-runtime-Cv_nyRjc.js";
import { t as createLocalizedCatalog } from "./localized-catalog-DubKHKUR.js";
import { n as translateSearchKeyword } from "./settings-search-keywords-Cutqc_5t.js";
const USAGE_PERCENTAGE_DISPLAY_SETTING_ID = "usage-percentage-display";
function resolveAppearanceAccordionDeepLink(sectionId) {
	if (sectionId === "usage-percentage-display") return "window";
	return null;
}
const getUsagePercentageDisplayEntry = createLocalizedCatalog(() => ({
	title: translate("auto.components.settings.appearance.search.usagePercentageDisplayTitle", "Usage percentages"),
	description: translate("auto.components.settings.appearance.search.usagePercentageDisplayDescription", "Choose whether provider limits show the percentage used or remaining."),
	keywords: [...translateSearchKeyword("auto.components.settings.appearance.search.00a028f25f", "usage"), ...translateSearchKeyword("auto.components.settings.appearance.search.896eb53fd4", "status bar")]
}));
export { getUsagePercentageDisplayEntry as n, resolveAppearanceAccordionDeepLink as r, USAGE_PERCENTAGE_DISPLAY_SETTING_ID as t };
