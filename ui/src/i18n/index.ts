import i18n from "i18next";
import { initReactI18next, useTranslation } from "react-i18next";

import en from "./en.json";

export const DEFAULT_LOCALE = "en";

export const resources = { en: { translation: en } } as const;

// English only until a second resource file exists, so there is no language detection. Init is
// synchronous (resources are bundled), so the first render already has its strings.
if (!i18n.isInitialized) {
  void i18n.use(initReactI18next).init({
    resources,
    lng: DEFAULT_LOCALE,
    fallbackLng: DEFAULT_LOCALE,
    initAsync: false,
    // React escapes rendered text; escaping here would double-encode "&" and ">".
    interpolation: { escapeValue: false },
  });
}

export { i18n, useTranslation };
