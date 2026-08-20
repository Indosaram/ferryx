import { n as getIntlLocale } from "./jsx-runtime-Cv_nyRjc.js";
var cached = null;
function getUiRelativeTimeFormatter() {
	const locale = getIntlLocale();
	if (!cached || cached.locale !== locale) cached = {
		locale,
		formatter: new Intl.RelativeTimeFormat(locale, { numeric: "auto" })
	};
	return cached.formatter;
}
function formatUiRelativeTime(diffMs) {
	const formatter = getUiRelativeTimeFormatter();
	const diffMinutes = Math.round(diffMs / 6e4);
	if (Math.abs(diffMinutes) < 60) return formatter.format(diffMinutes, "minute");
	const diffHours = Math.round(diffMinutes / 60);
	if (Math.abs(diffHours) < 24) return formatter.format(diffHours, "hour");
	return formatter.format(Math.round(diffHours / 24), "day");
}
function formatUiRelativeTimeFromDate(input, fallback = "recently") {
	const date = new Date(input);
	if (Number.isNaN(date.getTime())) return fallback;
	return formatUiRelativeTime(date.getTime() - Date.now());
}
export { formatUiRelativeTimeFromDate as n, formatUiRelativeTime as t };
