// Ported from pingdotgg/t3code (MIT) packages/shared/src/themePalettes.ts,
// packages/shared/src/themePreview.ts, and apps/web/src/themePalette.ts.
// Pure zero-dependency OKLCH palette derivation, curated T3 Code presets, and Ferryx token mapping.

import { parseThemeColor, THEME_TOKEN_ALLOWLIST, type ThemeDocumentV1 } from "./themeDocument";

export const CURATED_THEME_IDS = ["t3-chat", "grove", "ocean", "ember", "iris"] as const;
export type CuratedThemeId = (typeof CURATED_THEME_IDS)[number];
export const THEME_FILE_VERSION = 1 as const;
export const THEME_CSS_MAX_LENGTH = 100_000;

export const RESERVED_THEME_IDS: ReadonlySet<string> = new Set([
  "system",
  "light",
  "dark",
  "charcoal",
  ...CURATED_THEME_IDS,
  "t3-chat-dark",
  "t3-grove",
  "t3-ocean",
  "t3-ember",
  "t3-iris",
  "t3-code",
]);

export type ThemeAppearance = "light" | "dark";
export type ThemeHalves = Readonly<{ light?: string; dark?: string }>;
export type ThemeColorOverrides = Readonly<Partial<Record<ThemeColorRole, string>>>;
export type ThemeVariantOverrides = Readonly<Partial<Record<ThemeAppearance, ThemeColorOverrides>>>;
export type ThemeCollection = Readonly<{ id: string; label: string }>;

export const THEME_COLOR_ROLES = [
  "canvas",
  "chrome",
  "toolbar",
  "toolbarForeground",
  "toolbarBorder",
  "toolbarControl",
  "toolbarControlForeground",
  "toolbarControlHover",
  "surface",
  "surfaceRaised",
  "surfaceOverlay",
  "text",
  "textMuted",
  "border",
  "input",
  "focus",
  "accent",
  "accentForeground",
  "secondary",
  "secondaryForeground",
  "muted",
  "mutedForeground",
  "placeholder",
  "secondaryLabel",
  "iconMuted",
  "error",
  "errorForeground",
  "errorSurface",
  "warning",
  "warningForeground",
  "warningSurface",
  "update",
  "updateForeground",
  "updateSurface",
  "accentSurface",
  "accentSurfaceForeground",
  "messageSurface",
  "messageForeground",
  "messageAction",
  "messageActionForeground",
  "messageActionHover",
  "codeBackground",
  "codeForeground",
  "searchMatchBackground",
  "searchMatchForeground",
  "searchMatchActiveBackground",
  "searchMatchActiveForeground",
  "sidebar",
  "sidebarForeground",
  "sidebarMutedForeground",
  "sidebarControlSurface",
  "sidebarRowHover",
  "sidebarRowActive",
  "sidebarRowSelected",
  "sidebarBorder",
  "terminalBackground",
  "terminalForeground",
  "terminalCursor",
  "terminalSelection",
  "terminalScrollbar",
  "terminalScrollbarHover",
] as const;

export type ThemeColorRole = (typeof THEME_COLOR_ROLES)[number];
export type ThemeColors = Readonly<Record<ThemeColorRole, string>>;
export type ThemeVariants = Readonly<Partial<Record<ThemeAppearance, ThemeColors>>>;
export type ThemeDefinition = Readonly<{
  id: string;
  label: string;
  appearance: ThemeAppearance;
  colors: ThemeColors;
  variants?: ThemeVariants;
  collection?: Readonly<{ id: string; label: string }>;
  sidebarArtwork?: boolean;
  managed?: boolean;
  /** Custom styles for web and desktop. */
  css?: string;
}>;

export const T3_CODE_LIGHT_THEME_COLORS: ThemeColors = {
  searchMatchBackground: "#ffde8b",
  searchMatchForeground: "#21180a",
  searchMatchActiveBackground: "#eea52b",
  searchMatchActiveForeground: "#21180a",
  canvas: "#fcfcfc",
  chrome: "#fcfcfc",
  toolbar: "#fcfcfc",
  toolbarForeground: "#27272a",
  toolbarBorder: "#e4e4e7",
  toolbarControl: "#ffffff",
  toolbarControlForeground: "#27272a",
  toolbarControlHover: "#f4f4f5",
  surface: "#ffffff",
  surfaceRaised: "#fcfcfc",
  surfaceOverlay: "#ffffff",
  text: "#27272a",
  textMuted: "#71717b",
  border: "#e4e4e7",
  input: "#d4d4d8",
  focus: "#1b4ed8",
  accent: "#1b4ed8",
  accentForeground: "#ffffff",
  secondary: "#fafafa",
  secondaryForeground: "#27272a",
  muted: "#fafafa",
  mutedForeground: "#71717b",
  placeholder: "#71717b",
  secondaryLabel: "#71717b",
  iconMuted: "#71717b",
  error: "#fb2c36",
  errorForeground: "#c10007",
  errorSurface: "#fcebec",
  warning: "#fe9a00",
  warningForeground: "#bb4d00",
  warningSurface: "#fcf4e8",
  update: "#1b4ed8",
  updateForeground: "#1b4ed8",
  updateSurface: "#e0e6f7",
  accentSurface: "#f4f4f5",
  accentSurfaceForeground: "#18181b",
  messageSurface: "#f4f4f5",
  messageForeground: "#27272a",
  messageAction: "#1b4ed8",
  messageActionForeground: "#ffffff",
  messageActionHover: "#3160db",
  codeBackground: "#ffffff",
  codeForeground: "#27272a",
  sidebar: "#fafafa",
  sidebarForeground: "#27272a",
  sidebarMutedForeground: "#71717b",
  sidebarControlSurface: "#f4f4f5",
  sidebarRowHover: "#fcfcfc",
  sidebarRowActive: "#ffffff",
  sidebarRowSelected: "#ffffff",
  sidebarBorder: "#e4e4e7",
  terminalBackground: "#fcfcfc",
  terminalForeground: "#27272a",
  terminalCursor: "#26384e",
  terminalSelection: "#d0d6dd",
  terminalScrollbar: "#d6d6d6",
  terminalScrollbarHover: "#bdbdbd",
};

export const T3_CODE_DARK_THEME_COLORS: ThemeColors = {
  searchMatchBackground: "#8c6c1f",
  searchMatchForeground: "#f5f5f5",
  searchMatchActiveBackground: "#eea52b",
  searchMatchActiveForeground: "#21180a",
  canvas: "#0a0a0a",
  chrome: "#0a0a0a",
  toolbar: "#0a0a0a",
  toolbarForeground: "#f5f5f5",
  toolbarBorder: "#191919",
  toolbarControl: "#111111",
  toolbarControlForeground: "#f5f5f5",
  toolbarControlHover: "#141414",
  surface: "#111111",
  surfaceRaised: "#111111",
  surfaceOverlay: "#111111",
  text: "#f5f5f5",
  textMuted: "#818181",
  border: "#191919",
  input: "#1e1e1e",
  focus: "#346bf1",
  accent: "#346bf1",
  accentForeground: "#ffffff",
  secondary: "#111111",
  secondaryForeground: "#f5f5f5",
  muted: "#111111",
  mutedForeground: "#818181",
  placeholder: "#818181",
  secondaryLabel: "#818181",
  iconMuted: "#818181",
  error: "#fb414a",
  errorForeground: "#ff6467",
  errorSurface: "#301214",
  warning: "#fe9a00",
  warningForeground: "#ffb900",
  warningSurface: "#312108",
  update: "#346bf1",
  updateForeground: "#51a2ff",
  updateSurface: "#121b34",
  accentSurface: "#141414",
  accentSurfaceForeground: "#f5f5f5",
  messageSurface: "#141414",
  messageForeground: "#f5f5f5",
  messageAction: "#346bf1",
  messageActionForeground: "#ffffff",
  messageActionHover: "#3061d9",
  codeBackground: "#111111",
  codeForeground: "#f5f5f5",
  sidebar: "#000000",
  sidebarForeground: "#f1f3f7",
  sidebarMutedForeground: "#a3a3a3",
  sidebarControlSurface: "#0a0a0a",
  sidebarRowHover: "#131313",
  sidebarRowActive: "#1a1b1b",
  sidebarRowSelected: "#111111",
  sidebarBorder: "#141414",
  terminalBackground: "#0a0a0a",
  terminalForeground: "#f5f5f5",
  terminalCursor: "#b4cbff",
  terminalSelection: "#343a47",
  terminalScrollbar: "#222222",
  terminalScrollbarHover: "#363636",
};

export const T3_CHAT_THEME: ThemeDefinition = {
  id: "t3-chat",
  label: "T3 Chat",
  appearance: "light",
  colors: {
    canvas: "oklch(0.982446 0.010114 325.653)",
    chrome: "oklch(0.982446 0.010114 325.653)",
    toolbar: "oklch(0.982446 0.010114 325.653)",
    toolbarForeground: "oklch(0.325698 0.116116 325.037)",
    toolbarBorder: "oklch(0.856784 0.082879 328.911)",
    toolbarControl: "oklch(0.939552 0.024286 321.664)",
    toolbarControlForeground: "oklch(0.325698 0.116116 325.037)",
    toolbarControlHover: "oklch(0.884525 0.041658 337.177)",
    surface: "oklch(0.971835 0.012884 321.894)",
    surfaceRaised: "oklch(0.988235 0.005049 325.615)",
    surfaceOverlay: "oklch(1 0 0)",
    text: "oklch(0.325698 0.116116 325.037)",
    textMuted: "oklch(0.494754 0.190937 354.544)",
    border: "oklch(0.923531 0.021247 328.096)",
    input: "oklch(0.851713 0.055822 336.6)",
    focus: "oklch(0.591646 0.217985 0.584)",
    accent: "oklch(0.591646 0.217985 0.584)",
    accentForeground: "oklch(1 0 0)",
    secondary: "oklch(0.869588 0.06751 334.899)",
    secondaryForeground: "oklch(0.444777 0.134061 324.799)",
    muted: "oklch(0.802407 0.090963 345.892)",
    mutedForeground: "oklch(0.428932 0.163929 354.332)",
    placeholder: "oklch(0.549927 0.090215 323.149)",
    secondaryLabel: "oklch(0.494754 0.190937 354.544)",
    iconMuted: "oklch(0.494754 0.190937 354.544)",
    error: "oklch(0.627117 0.248974 7.734)",
    errorForeground: "oklch(0.458704 0.169677 3.815)",
    errorSurface: "oklch(0.942787 0.032076 344.963)",
    warning: "oklch(0.76859 0.164659 70.08)",
    warningForeground: "oklch(0.54612 0.143036 48.949)",
    warningSurface: "oklch(0.962901 0.015297 48.56)",
    update: "oklch(0.591646 0.217985 0.584)",
    updateForeground: "oklch(0.494754 0.190937 354.544)",
    updateSurface: "oklch(0.930264 0.036194 341.45)",
    accentSurface: "oklch(0.939552 0.024286 321.664)",
    accentSurfaceForeground: "oklch(0.396296 0.025134 285.196)",
    messageSurface: "oklch(0.926746 0.037898 332.6)",
    messageForeground: "oklch(0.354591 0.093575 307.568)",
    messageAction: "oklch(0.591646 0.217985 0.584)",
    messageActionForeground: "oklch(1 0 0)",
    messageActionHover: "oklch(0.539042 0.197866 0.305)",
    codeBackground: "oklch(0.953855 0.019695 315.668)",
    codeForeground: "oklch(0.445128 0.13005 307.026)",
    searchMatchBackground: "oklch(0.915 0.11 85)",
    searchMatchForeground: "oklch(0.216164 0.028283 77.423)",
    searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
    searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
    sidebar: "oklch(0.928886 0.031178 322.592)",
    sidebarForeground: "oklch(0.396296 0.025134 285.196)",
    sidebarMutedForeground: "oklch(0.494754 0.190937 354.544)",
    sidebarControlSurface: "oklch(0.978851 0.001321 106.424)",
    sidebarRowHover: "oklch(0.978851 0.001321 106.424)",
    sidebarRowActive: "oklch(0.978851 0.001321 106.424)",
    sidebarRowSelected: "oklch(0.978851 0.001321 106.424)",
    sidebarBorder: "oklch(0.938313 0.002552 48.717)",
    terminalBackground: "oklch(0.982446 0.010114 325.653)",
    terminalForeground: "oklch(0.325698 0.116116 325.037)",
    terminalCursor: "oklch(0.591646 0.217985 0.584)",
    terminalSelection: "oklch(0.869588 0.06751 334.899)",
    terminalScrollbar: "oklch(0.851713 0.055822 336.6)",
    terminalScrollbarHover: "oklch(0.802407 0.090963 345.892)",
  },
  variants: {
    dark: {
      canvas: "oklch(0.22813 0.020366 307.469)",
      chrome: "oklch(0.22813 0.020366 307.469)",
      toolbar: "oklch(0.22813 0.020366 307.469)",
      toolbarForeground: "oklch(0.980735 0.004092 301.426)",
      toolbarBorder: "oklch(0.266943 0.015262 302.425)",
      toolbarControl: "oklch(0.313674 0.030572 310.061)",
      toolbarControlForeground: "oklch(0.848252 0.038248 307.961)",
      toolbarControlHover: "oklch(0.364912 0.050794 308.491)",
      surface: "oklch(0.267101 0.02016 311.799)",
      surfaceRaised: "oklch(0.279864 0.021572 309.532)",
      surfaceOverlay: "oklch(0.154761 0.01316 338.901)",
      text: "oklch(0.980735 0.004092 301.426)",
      textMuted: "oklch(0.880303 0.03077 342.696)",
      border: "oklch(0.266943 0.015262 302.425)",
      input: "oklch(0.266817 0.02897 344.461)",
      focus: "oklch(0.591646 0.217985 0.584)",
      accent: "oklch(0.460685 0.185347 4.099)",
      accentForeground: "oklch(0.901233 0.057189 343.694)",
      secondary: "oklch(0.313674 0.030572 310.061)",
      secondaryForeground: "oklch(0.848252 0.038248 307.961)",
      muted: "oklch(0.360924 0.021469 316.83)",
      mutedForeground: "oklch(0.880303 0.03077 342.696)",
      placeholder: "oklch(0.657087 0.028226 307.985)",
      secondaryLabel: "oklch(0.880303 0.03077 342.696)",
      iconMuted: "oklch(0.848252 0.038248 307.961)",
      error: "oklch(0.458704 0.169677 3.815)",
      errorForeground: "oklch(0.901233 0.057189 343.694)",
      errorSurface: "oklch(0.259022 0.04799 340.062)",
      warning: "oklch(0.76859 0.164659 70.08)",
      warningForeground: "oklch(0.836861 0.164422 84.429)",
      warningSurface: "oklch(0.321706 0.036256 60.806)",
      update: "oklch(0.460685 0.185347 4.099)",
      updateForeground: "oklch(0.901233 0.057189 343.694)",
      updateSurface: "oklch(0.256077 0.063004 342.914)",
      accentSurface: "oklch(0.364912 0.050794 308.491)",
      accentSurfaceForeground: "oklch(0.964695 0.009139 341.803)",
      messageSurface: "oklch(0.273791 0.025541 309.079)",
      messageForeground: "oklch(0.949872 0.021269 306.838)",
      messageAction: "oklch(0.460685 0.185347 4.099)",
      messageActionForeground: "oklch(0.901233 0.057189 343.694)",
      messageActionHover: "oklch(0.458754 0.184639 3.857)",
      codeBackground: "oklch(0.22813 0.020366 307.469)",
      codeForeground: "oklch(0.848703 0.064239 306.645)",
      searchMatchBackground: "oklch(0.55 0.1 85)",
      searchMatchForeground: "oklch(0.970151 0 0)",
      searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
      searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
      sidebar: "oklch(0.185778 0.019368 322.159)",
      sidebarForeground: "oklch(0.967434 0.001326 286.375)",
      sidebarMutedForeground: "oklch(0.880303 0.03077 342.696)",
      sidebarControlSurface: "oklch(0.23366 0.026081 338.196)",
      sidebarRowHover: "oklch(0.23366 0.026081 338.196)",
      sidebarRowActive: "oklch(0.23366 0.026081 338.196)",
      sidebarRowSelected: "oklch(0.23366 0.026081 338.196)",
      sidebarBorder: "oklch(0.269132 0.030766 351.067)",
      terminalBackground: "oklch(0.22813 0.020366 307.469)",
      terminalForeground: "oklch(0.980735 0.004092 301.426)",
      terminalCursor: "oklch(0.591646 0.217985 0.584)",
      terminalSelection: "oklch(0.313674 0.030572 310.061)",
      terminalScrollbar: "oklch(0.266817 0.02897 344.461)",
      terminalScrollbarHover: "oklch(0.360924 0.021469 316.83)",
    },
  },
  sidebarArtwork: true,
};

export const GROVE_THEME: ThemeDefinition = {
  id: "grove",
  label: "Grove",
  appearance: "light",
  colors: {
    canvas: "oklch(0.972369 0.005497 157.15)",
    chrome: "oklch(0.972369 0.005497 157.15)",
    toolbar: "oklch(0.972369 0.005497 157.15)",
    toolbarForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarBorder: "oklch(0.909438 0.021521 164.612)",
    toolbarControl: "oklch(0.936464 0.014601 163.554)",
    toolbarControlForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarControlHover: "oklch(0.909438 0.021521 164.612)",
    surface: "oklch(0.972369 0.005497 157.15)",
    surfaceRaised: "oklch(0.949276 0.004496 159.002)",
    surfaceOverlay: "oklch(0.932695 0.003778 160.944)",
    text: "oklch(0.222003 0.03479 328.979)",
    textMuted: "oklch(0.540472 0.014944 326.176)",
    border: "oklch(0.864831 0.01312 167.255)",
    input: "oklch(0.829746 0.016084 168.234)",
    focus: "oklch(0.523295 0.112292 158.089)",
    accent: "oklch(0.523295 0.112292 158.089)",
    accentForeground: "oklch(0.990339 0.008411 325.64)",
    secondary: "oklch(0.936464 0.014601 163.554)",
    secondaryForeground: "oklch(0.222003 0.03479 328.979)",
    muted: "oklch(0.945455 0.012308 162.879)",
    mutedForeground: "oklch(0.527266 0.012309 320.683)",
    placeholder: "oklch(0.529681 0.01551 326.299)",
    secondaryLabel: "oklch(0.540472 0.014944 326.176)",
    iconMuted: "oklch(0.540472 0.014944 326.176)",
    error: "oklch(0.637823 0.237287 25.436)",
    errorForeground: "oklch(0.509494 0.208583 28.513)",
    errorSurface: "oklch(0.936968 0.014243 26.295)",
    warning: "oklch(0.772406 0.172798 65.367)",
    warningForeground: "oklch(0.545036 0.155019 45.359)",
    warningSurface: "oklch(0.953175 0.02009 93.379)",
    update: "oklch(0.523295 0.112292 158.089)",
    updateForeground: "oklch(0.388012 0.080082 158.768)",
    updateSurface: "oklch(0.900411 0.02384 164.795)",
    accentSurface: "oklch(0.909438 0.021521 164.612)",
    accentSurfaceForeground: "oklch(0.222003 0.03479 328.979)",
    messageSurface: "oklch(0.891377 0.026164 164.929)",
    messageForeground: "oklch(0.222003 0.03479 328.979)",
    messageAction: "oklch(0.535028 0.106403 77.549)",
    messageActionForeground: "oklch(0.990339 0.008411 325.64)",
    messageActionHover: "oklch(0.488753 0.096536 77.829)",
    codeBackground: "oklch(0.955888 0.004783 158.391)",
    codeForeground: "oklch(0.222003 0.03479 328.979)",
    searchMatchBackground: "oklch(0.915 0.11 85)",
    searchMatchForeground: "oklch(0.216164 0.028283 77.423)",
    searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
    searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
    sidebar: "oklch(0.936464 0.014601 163.554)",
    sidebarForeground: "oklch(0.222003 0.03479 328.979)",
    sidebarMutedForeground: "oklch(0.515606 0.011938 318.897)",
    sidebarControlSurface: "oklch(0.88585 0.011734 166.331)",
    sidebarRowHover: "oklch(0.886676 0.027374 164.983)",
    sidebarRowActive: "oklch(0.85335 0.03597 165.158)",
    sidebarRowSelected: "oklch(0.836654 0.040284 165.149)",
    sidebarBorder: "oklch(0.860274 0.010287 168.339)",
    terminalBackground: "oklch(0.972369 0.005497 157.15)",
    terminalForeground: "oklch(0.222003 0.03479 328.979)",
    terminalCursor: "oklch(0.523295 0.112292 158.089)",
    terminalSelection: "oklch(0.891377 0.026164 164.929)",
    terminalScrollbar: "oklch(0.824752 0.001392 294.641)",
    terminalScrollbarHover: "oklch(0.755495 0.004415 318.776)",
  },
  variants: {
    dark: {
      canvas: "oklch(0.260865 0.02152 162.75)",
      chrome: "oklch(0.260865 0.02152 162.75)",
      toolbar: "oklch(0.260865 0.02152 162.75)",
      toolbarForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarBorder: "oklch(0.464636 0.066083 158.72)",
      toolbarControl: "oklch(0.380487 0.048313 159.608)",
      toolbarControlForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarControlHover: "oklch(0.437021 0.060312 158.962)",
      surface: "oklch(0.260865 0.02152 162.75)",
      surfaceRaised: "oklch(0.363192 0.016572 165.32)",
      surfaceOverlay: "oklch(0.411828 0.014378 166.627)",
      text: "oklch(0.990339 0.008411 325.64)",
      textMuted: "oklch(0.666747 0.004239 187.292)",
      border: "oklch(0.457475 0.044046 160.971)",
      input: "oklch(0.519849 0.049896 160.863)",
      focus: "oklch(0.796228 0.133058 157.319)",
      accent: "oklch(0.796228 0.133058 157.319)",
      accentForeground: "oklch(0.222003 0.03479 328.979)",
      secondary: "oklch(0.380487 0.048313 159.608)",
      secondaryForeground: "oklch(0.990339 0.008411 325.64)",
      muted: "oklch(0.339728 0.039456 160.274)",
      mutedForeground: "oklch(0.715427 0.010896 171.428)",
      placeholder: "oklch(0.739243 0.002222 223.225)",
      secondaryLabel: "oklch(0.666747 0.004239 187.292)",
      iconMuted: "oklch(0.666747 0.004239 187.292)",
      error: "oklch(0.655108 0.221148 23.473)",
      errorForeground: "oklch(0.704237 0.187511 22.228)",
      errorSurface: "oklch(0.312773 0.02923 32.121)",
      warning: "oklch(0.772406 0.172798 65.367)",
      warningForeground: "oklch(0.829017 0.171221 81.038)",
      warningSurface: "oklch(0.345524 0.046882 99.736)",
      update: "oklch(0.796228 0.133058 157.319)",
      updateForeground: "oklch(0.86276 0.089288 159.704)",
      updateSurface: "oklch(0.448116 0.062637 158.86)",
      accentSurface: "oklch(0.437021 0.060312 158.962)",
      accentSurfaceForeground: "oklch(0.990339 0.008411 325.64)",
      messageSurface: "oklch(0.470111 0.067221 158.676)",
      messageForeground: "oklch(0.990339 0.008411 325.64)",
      messageAction: "oklch(0.791603 0.129713 83.299)",
      messageActionForeground: "oklch(0.222003 0.03479 328.979)",
      messageActionHover: "oklch(0.815227 0.117902 84.21)",
      codeBackground: "oklch(0.312979 0.018942 164.082)",
      codeForeground: "oklch(0.990339 0.008411 325.64)",
      searchMatchBackground: "oklch(0.55 0.1 85)",
      searchMatchForeground: "oklch(0.970151 0 0)",
      searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
      searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
      sidebar: "oklch(0.309925 0.032827 160.944)",
      sidebarForeground: "oklch(0.990339 0.008411 325.64)",
      sidebarMutedForeground: "oklch(0.711387 0.007643 175.89)",
      sidebarControlSurface: "oklch(0.432727 0.024549 163.654)",
      sidebarRowHover: "oklch(0.374959 0.047124 159.686)",
      sidebarRowActive: "oklch(0.41688 0.056069 159.165)",
      sidebarRowSelected: "oklch(0.437466 0.060406 158.958)",
      sidebarBorder: "oklch(0.569253 0.015933 167.062)",
      terminalBackground: "oklch(0.260865 0.02152 162.75)",
      terminalForeground: "oklch(0.990339 0.008411 325.64)",
      terminalCursor: "oklch(0.796228 0.133058 157.319)",
      terminalSelection: "oklch(0.464636 0.066083 158.72)",
      terminalScrollbar: "oklch(0.594692 0.006862 176.022)",
      terminalScrollbarHover: "oklch(0.687968 0.00354 193.55)",
    },
  },
  sidebarArtwork: true,
};

export const OCEAN_THEME: ThemeDefinition = {
  id: "ocean",
  label: "Ocean",
  appearance: "light",
  colors: {
    canvas: "oklch(0.974199 0.002856 241.597)",
    chrome: "oklch(0.974199 0.002856 241.597)",
    toolbar: "oklch(0.974199 0.002856 241.597)",
    toolbarForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarBorder: "oklch(0.91295 0.018827 241.836)",
    toolbarControl: "oklch(0.939254 0.01193 241.729)",
    toolbarControlForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarControlHover: "oklch(0.91295 0.018827 241.836)",
    surface: "oklch(0.974199 0.002856 241.597)",
    surfaceRaised: "oklch(0.951058 0.002962 258.339)",
    surfaceOverlay: "oklch(0.934442 0.003181 269.1)",
    text: "oklch(0.222003 0.03479 328.979)",
    textMuted: "oklch(0.541555 0.017468 323.531)",
    border: "oklch(0.867646 0.013482 252.362)",
    input: "oklch(0.832939 0.017389 252.598)",
    focus: "oklch(0.536684 0.120219 247.01)",
    accent: "oklch(0.536684 0.120219 247.01)",
    accentForeground: "oklch(0.990339 0.008411 325.64)",
    secondary: "oklch(0.939254 0.01193 241.729)",
    secondaryForeground: "oklch(0.222003 0.03479 328.979)",
    muted: "oklch(0.948004 0.009649 241.695)",
    mutedForeground: "oklch(0.528741 0.01828 313.823)",
    placeholder: "oklch(0.530733 0.01795 323.79)",
    secondaryLabel: "oklch(0.541555 0.017468 323.531)",
    iconMuted: "oklch(0.541555 0.017468 323.531)",
    error: "oklch(0.637823 0.237287 25.436)",
    errorForeground: "oklch(0.509494 0.208583 28.513)",
    errorSurface: "oklch(0.938747 0.016377 7.186)",
    warning: "oklch(0.772406 0.172798 65.367)",
    warningForeground: "oklch(0.546927 0.155556 45.359)",
    warningSurface: "oklch(0.954846 0.016009 81.731)",
    update: "oklch(0.536684 0.120219 247.01)",
    updateForeground: "oklch(0.397497 0.084999 246.523)",
    updateSurface: "oklch(0.904165 0.021144 241.874)",
    accentSurface: "oklch(0.91295 0.018827 241.836)",
    accentSurfaceForeground: "oklch(0.222003 0.03479 328.979)",
    messageSurface: "oklch(0.895373 0.023469 241.913)",
    messageForeground: "oklch(0.222003 0.03479 328.979)",
    messageAction: "oklch(0.493961 0.08175 201.584)",
    messageActionForeground: "oklch(0.990339 0.008411 325.64)",
    messageActionHover: "oklch(0.45151 0.074407 201.516)",
    codeBackground: "oklch(0.957684 0.002906 253.68)",
    codeForeground: "oklch(0.222003 0.03479 328.979)",
    searchMatchBackground: "oklch(0.915 0.11 85)",
    searchMatchForeground: "oklch(0.216164 0.028283 77.423)",
    searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
    searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
    sidebar: "oklch(0.939254 0.01193 241.729)",
    sidebarForeground: "oklch(0.222003 0.03479 328.979)",
    sidebarMutedForeground: "oklch(0.517366 0.018944 311.433)",
    sidebarControlSurface: "oklch(0.888479 0.011475 251.638)",
    sidebarRowHover: "oklch(0.890798 0.024681 241.933)",
    sidebarRowActive: "oklch(0.858363 0.033325 242.089)",
    sidebarRowSelected: "oklch(0.842113 0.037689 242.174)",
    sidebarBorder: "oklch(0.862823 0.011384 256.926)",
    terminalBackground: "oklch(0.974199 0.002856 241.597)",
    terminalForeground: "oklch(0.222003 0.03479 328.979)",
    terminalCursor: "oklch(0.536684 0.120219 247.01)",
    terminalSelection: "oklch(0.895373 0.023469 241.913)",
    terminalScrollbar: "oklch(0.826271 0.006191 305.456)",
    terminalScrollbarHover: "oklch(0.756866 0.008685 313.721)",
  },
  variants: {
    dark: {
      canvas: "oklch(0.242641 0.024125 250.573)",
      chrome: "oklch(0.242641 0.024125 250.573)",
      toolbar: "oklch(0.242641 0.024125 250.573)",
      toolbarForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarBorder: "oklch(0.439946 0.0561 243.479)",
      toolbarControl: "oklch(0.358725 0.043145 244.911)",
      toolbarControlForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarControlHover: "oklch(0.413315 0.051874 243.855)",
      surface: "oklch(0.242641 0.024125 250.573)",
      surfaceRaised: "oklch(0.348439 0.019942 253.696)",
      surfaceOverlay: "oklch(0.398517 0.018232 255.72)",
      text: "oklch(0.990339 0.008411 325.64)",
      textMuted: "oklch(0.652227 0.01149 273.31)",
      border: "oklch(0.438653 0.039496 245.44)",
      input: "oklch(0.500905 0.043574 244.781)",
      focus: "oklch(0.758933 0.105833 241.548)",
      accent: "oklch(0.758933 0.105833 241.548)",
      accentForeground: "oklch(0.222003 0.03479 328.979)",
      secondary: "oklch(0.358725 0.043145 244.911)",
      secondaryForeground: "oklch(0.990339 0.008411 325.64)",
      muted: "oklch(0.319287 0.036766 246.065)",
      mutedForeground: "oklch(0.691936 0.016294 261.588)",
      placeholder: "oklch(0.721641 0.010192 281.271)",
      secondaryLabel: "oklch(0.652227 0.01149 273.31)",
      iconMuted: "oklch(0.652227 0.01149 273.31)",
      error: "oklch(0.655108 0.221148 23.473)",
      errorForeground: "oklch(0.702184 0.189226 22.228)",
      errorSurface: "oklch(0.298933 0.036443 350.094)",
      warning: "oklch(0.772406 0.172798 65.367)",
      warningForeground: "oklch(0.829017 0.171221 81.038)",
      warningSurface: "oklch(0.329449 0.028712 84.495)",
      update: "oklch(0.758933 0.105833 241.548)",
      updateForeground: "oklch(0.840844 0.069217 240.151)",
      updateSurface: "oklch(0.424017 0.053575 243.695)",
      accentSurface: "oklch(0.413315 0.051874 243.855)",
      accentSurfaceForeground: "oklch(0.990339 0.008411 325.64)",
      messageSurface: "oklch(0.445224 0.056936 243.413)",
      messageForeground: "oklch(0.990339 0.008411 325.64)",
      messageAction: "oklch(0.793363 0.105022 199.893)",
      messageActionForeground: "oklch(0.222003 0.03479 328.979)",
      messageActionHover: "oklch(0.815308 0.096174 199.862)",
      codeBackground: "oklch(0.29661 0.021883 251.968)",
      codeForeground: "oklch(0.990339 0.008411 325.64)",
      searchMatchBackground: "oklch(0.55 0.1 85)",
      searchMatchForeground: "oklch(0.970151 0 0)",
      searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
      searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
      sidebar: "oklch(0.290387 0.032043 247.274)",
      sidebarForeground: "oklch(0.990339 0.008411 325.64)",
      sidebarMutedForeground: "oklch(0.69099 0.01395 266.424)",
      sidebarControlSurface: "oklch(0.417822 0.02535 250.162)",
      sidebarRowHover: "oklch(0.353381 0.042285 245.043)",
      sidebarRowActive: "oklch(0.393878 0.048778 244.179)",
      sidebarRowSelected: "oklch(0.413744 0.051943 243.848)",
      sidebarBorder: "oklch(0.55859 0.019001 256.223)",
      terminalBackground: "oklch(0.242641 0.024125 250.573)",
      terminalForeground: "oklch(0.990339 0.008411 325.64)",
      terminalCursor: "oklch(0.758933 0.105833 241.548)",
      terminalSelection: "oklch(0.439946 0.0561 243.479)",
      terminalScrollbar: "oklch(0.58613 0.012959 267.22)",
      terminalScrollbarHover: "oklch(0.681569 0.010909 276.465)",
    },
  },
  sidebarArtwork: true,
};

export const EMBER_THEME: ThemeDefinition = {
  id: "ember",
  label: "Ember",
  appearance: "light",
  colors: {
    canvas: "oklch(0.976527 0.002685 60.725)",
    chrome: "oklch(0.976527 0.002685 60.725)",
    toolbar: "oklch(0.976527 0.002685 60.725)",
    toolbarForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarBorder: "oklch(0.916502 0.01832 49.597)",
    toolbarControl: "oklch(0.942267 0.01151 50.785)",
    toolbarControlForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarControlHover: "oklch(0.916502 0.01832 49.597)",
    surface: "oklch(0.976527 0.002685 60.725)",
    surfaceRaised: "oklch(0.953321 0.002701 42.266)",
    surfaceOverlay: "oklch(0.936659 0.002879 29.96)",
    text: "oklch(0.222003 0.03479 328.979)",
    textMuted: "oklch(0.543023 0.017316 331.964)",
    border: "oklch(0.870631 0.013204 39.431)",
    input: "oklch(0.836213 0.017153 38.661)",
    focus: "oklch(0.552831 0.129438 44.656)",
    accent: "oklch(0.552831 0.129438 44.656)",
    accentForeground: "oklch(0.990339 0.008411 325.64)",
    secondary: "oklch(0.942267 0.01151 50.785)",
    secondaryForeground: "oklch(0.222003 0.03479 328.979)",
    muted: "oklch(0.950842 0.009273 51.528)",
    mutedForeground: "oklch(0.530413 0.018453 341.181)",
    placeholder: "oklch(0.532339 0.017796 331.748)",
    secondaryLabel: "oklch(0.543023 0.017316 331.964)",
    iconMuted: "oklch(0.543023 0.017316 331.964)",
    error: "oklch(0.637823 0.237287 25.436)",
    errorForeground: "oklch(0.509494 0.208583 28.513)",
    errorSurface: "oklch(0.941094 0.019938 19.375)",
    warning: "oklch(0.772406 0.172798 65.367)",
    warningForeground: "oklch(0.549154 0.156188 45.359)",
    warningSurface: "oklch(0.957148 0.020843 76.702)",
    update: "oklch(0.552831 0.129438 44.656)",
    updateForeground: "oklch(0.408647 0.091207 45.037)",
    updateSurface: "oklch(0.907902 0.020621 49.36)",
    accentSurface: "oklch(0.916502 0.01832 49.597)",
    accentSurfaceForeground: "oklch(0.222003 0.03479 328.979)",
    messageSurface: "oklch(0.899296 0.022939 49.163)",
    messageForeground: "oklch(0.222003 0.03479 328.979)",
    messageAction: "oklch(0.516323 0.161628 24.82)",
    messageActionForeground: "oklch(0.990339 0.008411 325.64)",
    messageActionHover: "oklch(0.471223 0.145843 24.688)",
    codeBackground: "oklch(0.959965 0.002668 47.512)",
    codeForeground: "oklch(0.222003 0.03479 328.979)",
    searchMatchBackground: "oklch(0.915 0.11 85)",
    searchMatchForeground: "oklch(0.216164 0.028283 77.423)",
    searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
    searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
    sidebar: "oklch(0.942267 0.01151 50.785)",
    sidebarForeground: "oklch(0.222003 0.03479 328.979)",
    sidebarMutedForeground: "oklch(0.519146 0.019214 343.427)",
    sidebarControlSurface: "oklch(0.891332 0.011179 40.596)",
    sidebarRowHover: "oklch(0.894819 0.024151 49.073)",
    sidebarRowActive: "oklch(0.863104 0.032855 48.586)",
    sidebarRowSelected: "oklch(0.84723 0.037292 48.403)",
    sidebarBorder: "oklch(0.865593 0.011154 35.246)",
    terminalBackground: "oklch(0.976527 0.002685 60.725)",
    terminalForeground: "oklch(0.222003 0.03479 328.979)",
    terminalCursor: "oklch(0.552831 0.129438 44.656)",
    terminalSelection: "oklch(0.899296 0.022939 49.163)",
    terminalScrollbar: "oklch(0.828185 0.005884 349.533)",
    terminalScrollbarHover: "oklch(0.758584 0.008423 341.16)",
  },
  variants: {
    dark: {
      canvas: "oklch(0.245899 0.019144 42.044)",
      chrome: "oklch(0.245899 0.019144 42.044)",
      toolbar: "oklch(0.245899 0.019144 42.044)",
      toolbarForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarBorder: "oklch(0.442681 0.0608 50.795)",
      toolbarControl: "oklch(0.361499 0.044052 49.515)",
      toolbarControlForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarControlHover: "oklch(0.416048 0.055354 50.484)",
      surface: "oklch(0.245899 0.019144 42.044)",
      surfaceRaised: "oklch(0.351262 0.01565 37.592)",
      surfaceOverlay: "oklch(0.401111 0.014308 34.896)",
      text: "oklch(0.990339 0.008411 325.64)",
      textMuted: "oklch(0.654017 0.009505 13.287)",
      border: "oklch(0.44099 0.040202 48.807)",
      input: "oklch(0.503003 0.045721 49.44)",
      focus: "oklch(0.762174 0.124117 52.082)",
      accent: "oklch(0.762174 0.124117 52.082)",
      accentForeground: "oklch(0.222003 0.03479 328.979)",
      secondary: "oklch(0.361499 0.044052 49.515)",
      secondaryForeground: "oklch(0.990339 0.008411 325.64)",
      muted: "oklch(0.322144 0.03574 48.309)",
      mutedForeground: "oklch(0.692479 0.015227 30.963)",
      placeholder: "oklch(0.723533 0.008741 4.515)",
      secondaryLabel: "oklch(0.654017 0.009505 13.287)",
      iconMuted: "oklch(0.654017 0.009505 13.287)",
      error: "oklch(0.655108 0.221148 23.473)",
      errorForeground: "oklch(0.702184 0.189226 22.228)",
      errorSurface: "oklch(0.310955 0.059624 24.334)",
      warning: "oklch(0.772406 0.172798 65.367)",
      warningForeground: "oklch(0.829017 0.171221 81.038)",
      warningSurface: "oklch(0.339137 0.055638 66.911)",
      update: "oklch(0.762174 0.124117 52.082)",
      updateForeground: "oklch(0.841456 0.079585 53.521)",
      updateSurface: "oklch(0.426749 0.057547 50.618)",
      accentSurface: "oklch(0.416048 0.055354 50.484)",
      accentSurfaceForeground: "oklch(0.990339 0.008411 325.64)",
      messageSurface: "oklch(0.447961 0.061874 50.849)",
      messageForeground: "oklch(0.990339 0.008411 325.64)",
      messageAction: "oklch(0.747955 0.135578 29.432)",
      messageActionForeground: "oklch(0.222003 0.03479 328.979)",
      messageActionHover: "oklch(0.775116 0.117953 29.014)",
      codeBackground: "oklch(0.299662 0.017229 39.973)",
      codeForeground: "oklch(0.990339 0.008411 325.64)",
      searchMatchBackground: "oklch(0.55 0.1 85)",
      searchMatchForeground: "oklch(0.970151 0 0)",
      searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
      searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
      sidebar: "oklch(0.293349 0.029554 46.882)",
      sidebarForeground: "oklch(0.990339 0.008411 325.64)",
      sidebarMutedForeground: "oklch(0.691874 0.012538 24.638)",
      sidebarControlSurface: "oklch(0.420227 0.022893 43.226)",
      sidebarRowHover: "oklch(0.356163 0.042933 49.385)",
      sidebarRowActive: "oklch(0.396617 0.051353 50.201)",
      sidebarRowSelected: "oklch(0.416477 0.055442 50.489)",
      sidebarBorder: "oklch(0.560372 0.016998 36.179)",
      terminalBackground: "oklch(0.245899 0.019144 42.044)",
      terminalForeground: "oklch(0.990339 0.008411 325.64)",
      terminalCursor: "oklch(0.762174 0.124117 52.082)",
      terminalSelection: "oklch(0.442681 0.0608 50.795)",
      terminalScrollbar: "oklch(0.587861 0.010463 20.444)",
      terminalScrollbarHover: "oklch(0.682876 0.009156 9.796)",
    },
  },
  sidebarArtwork: true,
};

export const IRIS_THEME: ThemeDefinition = {
  id: "iris",
  label: "Iris",
  appearance: "light",
  colors: {
    canvas: "oklch(0.976531 0.003855 303.226)",
    chrome: "oklch(0.976531 0.003855 303.226)",
    toolbar: "oklch(0.976531 0.003855 303.226)",
    toolbarForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarBorder: "oklch(0.914882 0.022965 299.986)",
    toolbarControl: "oklch(0.941387 0.014687 300.474)",
    toolbarControlForeground: "oklch(0.222003 0.03479 328.979)",
    toolbarControlHover: "oklch(0.914882 0.022965 299.986)",
    surface: "oklch(0.976531 0.003855 303.226)",
    surfaceRaised: "oklch(0.953326 0.004536 307.676)",
    surfaceOverlay: "oklch(0.936665 0.005041 310.132)",
    text: "oklch(0.222003 0.03479 328.979)",
    textMuted: "oklch(0.543042 0.018894 325.652)",
    border: "oklch(0.869608 0.018226 303.859)",
    input: "oklch(0.834773 0.023405 303.676)",
    focus: "oklch(0.525348 0.15373 294.176)",
    accent: "oklch(0.525348 0.15373 294.176)",
    accentForeground: "oklch(0.990339 0.008411 325.64)",
    secondary: "oklch(0.941387 0.014687 300.474)",
    secondaryForeground: "oklch(0.222003 0.03479 328.979)",
    muted: "oklch(0.950194 0.011956 300.733)",
    mutedForeground: "oklch(0.529955 0.022319 321.556)",
    placeholder: "oklch(0.532177 0.019333 325.784)",
    secondaryLabel: "oklch(0.543042 0.018894 325.652)",
    iconMuted: "oklch(0.543042 0.018894 325.652)",
    error: "oklch(0.637823 0.237287 25.436)",
    errorForeground: "oklch(0.509494 0.208583 28.513)",
    errorSurface: "oklch(0.941043 0.019582 4.235)",
    warning: "oklch(0.772406 0.172798 65.367)",
    warningForeground: "oklch(0.549154 0.156188 45.359)",
    warningSurface: "oklch(0.957054 0.016197 69.932)",
    update: "oklch(0.525348 0.15373 294.176)",
    updateForeground: "oklch(0.389926 0.10825 294.547)",
    updateSurface: "oklch(0.90602 0.025754 299.867)",
    accentSurface: "oklch(0.914882 0.022965 299.986)",
    accentSurfaceForeground: "oklch(0.222003 0.03479 328.979)",
    messageSurface: "oklch(0.897143 0.028558 299.758)",
    messageForeground: "oklch(0.222003 0.03479 328.979)",
    messageAction: "oklch(0.516084 0.185229 340.776)",
    messageActionForeground: "oklch(0.990339 0.008411 325.64)",
    messageActionHover: "oklch(0.471003 0.16748 340.687)",
    codeBackground: "oklch(0.95997 0.004338 306.542)",
    codeForeground: "oklch(0.222003 0.03479 328.979)",
    searchMatchBackground: "oklch(0.915 0.11 85)",
    searchMatchForeground: "oklch(0.216164 0.028283 77.423)",
    searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
    searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
    sidebar: "oklch(0.941387 0.014687 300.474)",
    sidebarForeground: "oklch(0.222003 0.03479 328.979)",
    sidebarMutedForeground: "oklch(0.518417 0.023683 320.681)",
    sidebarControlSurface: "oklch(0.890512 0.0155 303.803)",
    sidebarRowHover: "oklch(0.892522 0.030022 299.704)",
    sidebarRowActive: "oklch(0.85971 0.040501 299.36)",
    sidebarRowSelected: "oklch(0.843236 0.045818 299.198)",
    sidebarBorder: "oklch(0.864805 0.015938 305.371)",
    terminalBackground: "oklch(0.976531 0.003855 303.226)",
    terminalForeground: "oklch(0.222003 0.03479 328.979)",
    terminalCursor: "oklch(0.525348 0.15373 294.176)",
    terminalSelection: "oklch(0.897143 0.028558 299.758)",
    terminalScrollbar: "oklch(0.828195 0.008526 318.858)",
    terminalScrollbarHover: "oklch(0.758596 0.010892 321.538)",
  },
  variants: {
    dark: {
      canvas: "oklch(0.225975 0.031062 293.741)",
      chrome: "oklch(0.225975 0.031062 293.741)",
      toolbar: "oklch(0.225975 0.031062 293.741)",
      toolbarForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarBorder: "oklch(0.395417 0.085554 294.182)",
      toolbarControl: "oklch(0.325405 0.063614 294.23)",
      toolbarControlForeground: "oklch(0.990339 0.008411 325.64)",
      toolbarControlHover: "oklch(0.372436 0.07841 294.204)",
      surface: "oklch(0.225975 0.031062 293.741)",
      surfaceRaised: "oklch(0.335291 0.026008 296.394)",
      surfaceOverlay: "oklch(0.386739 0.024023 297.509)",
      text: "oklch(0.990339 0.008411 325.64)",
      textMuted: "oklch(0.640465 0.016197 304.171)",
      border: "oklch(0.40874 0.058536 295.893)",
      input: "oklch(0.46756 0.065775 296.265)",
      focus: "oklch(0.671712 0.169136 293.929)",
      accent: "oklch(0.671712 0.169136 293.929)",
      accentForeground: "oklch(0.222003 0.03479 328.979)",
      secondary: "oklch(0.325405 0.063614 294.23)",
      secondaryForeground: "oklch(0.990339 0.008411 325.64)",
      muted: "oklch(0.291515 0.05276 294.209)",
      mutedForeground: "oklch(0.663321 0.025932 301.862)",
      placeholder: "oklch(0.706249 0.014508 306.607)",
      secondaryLabel: "oklch(0.640465 0.016197 304.171)",
      iconMuted: "oklch(0.640465 0.016197 304.171)",
      error: "oklch(0.655108 0.221148 23.473)",
      errorForeground: "oklch(0.702184 0.189226 22.228)",
      errorSurface: "oklch(0.291658 0.054707 352.238)",
      warning: "oklch(0.772406 0.172798 65.367)",
      warningForeground: "oklch(0.829017 0.171221 81.038)",
      warningSurface: "oklch(0.318952 0.033845 51.646)",
      update: "oklch(0.671712 0.169136 293.929)",
      updateForeground: "oklch(0.785032 0.108439 296.344)",
      updateSurface: "oklch(0.381668 0.081286 294.195)",
      accentSurface: "oklch(0.372436 0.07841 294.204)",
      accentSurfaceForeground: "oklch(0.990339 0.008411 325.64)",
      messageSurface: "oklch(0.399975 0.086965 294.177)",
      messageForeground: "oklch(0.990339 0.008411 325.64)",
      messageAction: "oklch(0.789904 0.130063 337.621)",
      messageActionForeground: "oklch(0.222003 0.03479 328.979)",
      messageActionHover: "oklch(0.813537 0.114101 337.23)",
      codeBackground: "oklch(0.281873 0.028308 295.193)",
      codeForeground: "oklch(0.990339 0.008411 325.64)",
      searchMatchBackground: "oklch(0.55 0.1 85)",
      searchMatchForeground: "oklch(0.970151 0 0)",
      searchMatchActiveBackground: "oklch(0.774202 0.151946 74.331)",
      searchMatchActiveForeground: "oklch(0.216164 0.028283 77.423)",
      sidebar: "oklch(0.266743 0.044689 294.138)",
      sidebarForeground: "oklch(0.990339 0.008411 325.64)",
      sidebarMutedForeground: "oklch(0.668773 0.021522 302.949)",
      sidebarControlSurface: "oklch(0.399977 0.035678 297.031)",
      sidebarRowHover: "oklch(0.320808 0.062152 294.23)",
      sidebarRowActive: "oklch(0.355677 0.073167 294.217)",
      sidebarRowSelected: "oklch(0.372806 0.078525 294.203)",
      sidebarBorder: "oklch(0.545895 0.027522 299.871)",
      terminalBackground: "oklch(0.225975 0.031062 293.741)",
      terminalForeground: "oklch(0.990339 0.008411 325.64)",
      terminalCursor: "oklch(0.671712 0.169136 293.929)",
      terminalSelection: "oklch(0.395417 0.085554 294.182)",
      terminalScrollbar: "oklch(0.578663 0.017888 302.229)",
      terminalScrollbarHover: "oklch(0.676012 0.015271 305.433)",
    },
  },
  sidebarArtwork: true,
};

export const BUILT_IN_THEMES: ReadonlyArray<ThemeDefinition> = [
  T3_CHAT_THEME,
  GROVE_THEME,
  OCEAN_THEME,
  EMBER_THEME,
  IRIS_THEME,
];

export const CURATED_THEMES: ReadonlyArray<ThemeDefinition> = BUILT_IN_THEMES;
const CURATED_THEME_MAP: ReadonlyMap<string, ThemeDefinition> = new Map(
  CURATED_THEMES.map((theme) => [theme.id, theme]),
);
const THEME_COLOR_ROLE_SET: ReadonlySet<string> = new Set(THEME_COLOR_ROLES);

export function isReservedThemeId(id: string): boolean {
  return RESERVED_THEME_IDS.has(id);
}

export function getCuratedTheme(id: string): ThemeDefinition | null {
  return CURATED_THEME_MAP.get(id) ?? null;
}

export function getStandardThemeColors(appearance: ThemeAppearance): ThemeColors {
  return appearance === "light" ? T3_CODE_LIGHT_THEME_COLORS : T3_CODE_DARK_THEME_COLORS;
}

export function getThemeModes(theme: Pick<ThemeDefinition, "appearance" | "variants">): ReadonlyArray<ThemeAppearance> {
  const modes: ThemeAppearance[] = [theme.appearance];
  const other: ThemeAppearance = theme.appearance === "light" ? "dark" : "light";
  if (theme.variants?.[other]) modes.push(other);
  return modes;
}

export function getThemeColorsForMode(theme: ThemeDefinition, appearance: ThemeAppearance): ThemeColors {
  if (theme.appearance === appearance) return theme.colors;
  return theme.variants?.[appearance] ?? theme.colors;
}

export type ThemeRgbColor = Readonly<{ r: number; g: number; b: number }>;
export type ThemeOklch = Readonly<{ L: number; C: number; h: number }>;

const THEME_LIGHT_FOREGROUND: ThemeRgbColor = { r: 250, g: 250, b: 250 };
const THEME_DARK_FOREGROUND: ThemeRgbColor = { r: 24, g: 24, b: 27 };
const THEME_WHITE_FOREGROUND: ThemeRgbColor = { r: 255, g: 255, b: 255 };
const THEME_BLACK_FOREGROUND: ThemeRgbColor = { r: 0, g: 0, b: 0 };

function clamp(v: number, min = 0, max = 1): number {
  return Math.min(max, Math.max(min, v));
}

function srgbToLinear(c: number): number {
  const v = c / 255;
  return v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
}

function linearToSrgb(v: number): number {
  const c = clamp(v, 0, 1);
  return c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055;
}

export function rgbToHex(rgb: ThemeRgbColor): string {
  const h = (n: number) => Math.round(clamp(n, 0, 255)).toString(16).padStart(2, "0");
  return `#${h(rgb.r)}${h(rgb.g)}${h(rgb.b)}`;
}

export function relativeLuminance(rgb: ThemeRgbColor): number {
  return 0.2126 * srgbToLinear(rgb.r) + 0.7152 * srgbToLinear(rgb.g) + 0.0722 * srgbToLinear(rgb.b);
}

export function contrastRatio(first: ThemeRgbColor, second: ThemeRgbColor): number {
  const a = relativeLuminance(first);
  const b = relativeLuminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}

export function mixRgb(first: ThemeRgbColor, second: ThemeRgbColor, amount: number): ThemeRgbColor {
  const t = clamp(amount, 0, 1);
  return {
    r: first.r * (1 - t) + second.r * t,
    g: first.g * (1 - t) + second.g * t,
    b: first.b * (1 - t) + second.b * t,
  };
}

export function rgbToOklch(rgb: ThemeRgbColor): ThemeOklch {
  const r = srgbToLinear(rgb.r);
  const g = srgbToLinear(rgb.g);
  const b = srgbToLinear(rgb.b);
  const lRoot = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const mRoot = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const sRoot = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * lRoot + 0.793617785 * mRoot - 0.0040720468 * sRoot;
  const a = 1.9779984951 * lRoot - 2.428592205 * mRoot + 0.4505937099 * sRoot;
  const ob = 0.0259040371 * lRoot + 0.7827717662 * mRoot - 0.808675766 * sRoot;
  const C = Math.hypot(a, ob);
  const h = C < 1e-6 ? 0 : ((Math.atan2(ob, a) * 180) / Math.PI + 360) % 360;
  return { L: clamp(L, 0, 1), C, h };
}

export function oklchToRgb(color: ThemeOklch, round = true): ThemeRgbColor {
  const rad = (color.h * Math.PI) / 180;
  const a = Math.max(0, color.C) * Math.cos(rad);
  const ob = Math.max(0, color.C) * Math.sin(rad);
  const L = clamp(color.L, 0, 1);
  const l = (L + 0.3963377774 * a + 0.2158037573 * ob) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * ob) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * ob) ** 3;
  const lr = 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s;
  const lg = -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s;
  const lb = -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s;
  const r = linearToSrgb(lr) * 255;
  const g = linearToSrgb(lg) * 255;
  const b = linearToSrgb(lb) * 255;
  return round ? { r: Math.round(r), g: Math.round(g), b: Math.round(b) } : { r, g, b };
}

function formatNum(value: number, digits: number): string {
  return Number(value.toFixed(digits)).toString();
}

export function oklchToCss(color: ThemeOklch): string {
  return `oklch(${formatNum(clamp(color.L, 0, 1), 6)} ${formatNum(Math.max(0, color.C), 6)} ${formatNum(((color.h % 360) + 360) % 360, 3)})`;
}

export function parseColorToRgb(value: string): ThemeRgbColor | null {
  const parsed = parseThemeColor(value);
  if (!parsed) return null;
  return { r: parsed[0], g: parsed[1], b: parsed[2] };
}

export function themeColorToHex(value: string): string | null {
  const rgb = parseColorToRgb(value);
  return rgb ? rgbToHex(rgb) : null;
}

export function toCanonicalThemeColor(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  const parsed = parseThemeColor(trimmed);
  if (!parsed) return null;
  return trimmed;
}

export function solveOklchLightness(
  color: ThemeOklch,
  surfaces: ReadonlyArray<ThemeRgbColor>,
  minContrast: number,
  direction: "lighter" | "darker",
): ThemeOklch {
  const passes = (L: number) => {
    const rgb = oklchToRgb({ ...color, L });
    return surfaces.every((surface) => contrastRatio(rgb, surface) >= minContrast);
  };
  if (passes(color.L)) return color;
  let lo = direction === "lighter" ? color.L : 0;
  let hi = direction === "lighter" ? 1 : color.L;
  const extreme = direction === "lighter" ? 1 : 0;
  if (!passes(extreme)) {
    return { L: extreme, C: 0, h: color.h };
  }
  let best = extreme;
  for (let i = 0; i < 16; i += 1) {
    const mid = (lo + hi) / 2;
    if (passes(mid)) {
      best = mid;
      if (direction === "lighter") hi = mid;
      else lo = mid;
    } else if (direction === "lighter") {
      lo = mid;
    } else {
      hi = mid;
    }
  }
  return { ...color, L: best };
}

/** Moves only lightness so a dim color keeps its hue while hitting minContrast on surface. */
export function readableThemeColorOn(color: string, surface: string, minContrast: number): string {
  const colorRgb = parseColorToRgb(color);
  const surfaceRgb = parseColorToRgb(surface);
  if (!colorRgb || !surfaceRgb) return color;
  if (contrastRatio(colorRgb, surfaceRgb) >= minContrast) return color;
  const direction = relativeLuminance(surfaceRgb) < 0.179 ? "lighter" : "darker";
  const solved = solveOklchLightness(rgbToOklch(colorRgb), [surfaceRgb], minContrast, direction);
  return rgbToHex(oklchToRgb(solved));
}

function readableThemeForeground(background: ThemeRgbColor): ThemeRgbColor {
  const lightContrast = contrastRatio(background, THEME_LIGHT_FOREGROUND);
  const darkContrast = contrastRatio(background, THEME_DARK_FOREGROUND);
  if (Math.max(lightContrast, darkContrast) >= 4.5) {
    return lightContrast >= darkContrast ? THEME_LIGHT_FOREGROUND : THEME_DARK_FOREGROUND;
  }
  return contrastRatio(background, THEME_WHITE_FOREGROUND) >=
    contrastRatio(background, THEME_BLACK_FOREGROUND)
    ? THEME_WHITE_FOREGROUND
    : THEME_BLACK_FOREGROUND;
}

function readableThemeText(
  background: ThemeRgbColor,
  foreground: ThemeRgbColor,
  amount: number,
  minimumRatio: number,
): ThemeRgbColor {
  const softened = mixRgb(foreground, background, amount);
  if (contrastRatio(softened, background) >= minimumRatio) return softened;
  let readable = foreground;
  let lower = 0;
  let upper = amount;
  for (let i = 0; i < 12; i += 1) {
    const mid = (lower + upper) / 2;
    const candidate = mixRgb(foreground, background, mid);
    if (contrastRatio(candidate, background) >= minimumRatio) {
      readable = candidate;
      lower = mid;
    } else {
      upper = mid;
    }
  }
  return readable;
}

function standardMutedThemeText(background: ThemeRgbColor, foreground: ThemeRgbColor): ThemeRgbColor {
  const target = relativeLuminance(background) < 0.179 ? 5.082 : 4.705;
  return readableThemeText(background, foreground, 1, target);
}

const STANDARD_STATUS_COLORS = {
  light: {
    error: "#fb2c36",
    errorForeground: "#c10007",
    warning: "#fe9a00",
    warningForeground: "#bb4d00",
  },
  dark: {
    error: "#fb414a",
    errorForeground: "#ff6467",
    warning: "#fe9a00",
    warningForeground: "#ffb900",
  },
} as const;

function standardStatusColors(canvas: ThemeRgbColor) {
  const appearance: ThemeAppearance = relativeLuminance(canvas) < 0.179 ? "dark" : "light";
  const standard = STANDARD_STATUS_COLORS[appearance];
  const surfaceMix = appearance === "dark" ? 0.16 : 0.08;
  const parseOr = (value: string) => parseColorToRgb(value) ?? canvas;
  const errorSurface = mixRgb(canvas, parseOr(standard.error), surfaceMix);
  const warningSurface = mixRgb(canvas, parseOr(standard.warning), surfaceMix);
  const readableOn = (fg: string, surface: ThemeRgbColor) =>
    rgbToHex(
      oklchToRgb(
        solveOklchLightness(
          rgbToOklch(parseOr(fg)),
          [surface],
          4.6,
          appearance === "dark" ? "lighter" : "darker",
        ),
      ),
    );
  return {
    error: standard.error,
    errorForeground: readableOn(standard.errorForeground, errorSurface),
    errorSurface: rgbToHex(errorSurface),
    warning: standard.warning,
    warningForeground: readableOn(standard.warningForeground, warningSurface),
    warningSurface: rgbToHex(warningSurface),
  };
}

/** Exact T3 Code OKLCH palette derivation from canvas + accent seed colors. */
export function createVividThemeColors(
  appearance: ThemeAppearance,
  backgroundValue: string,
  accentValue: string,
): ThemeColors {
  const defaults = getStandardThemeColors(appearance);
  const canvasRgb =
    parseColorToRgb(backgroundValue) ??
    (appearance === "dark" ? { r: 24, g: 15, b: 27 } : { r: 250, g: 245, b: 250 });
  const accentRgb = parseColorToRgb(accentValue) ?? { r: 168, g: 67, b: 112 };
  const canvas = rgbToOklch(canvasRgb);
  const accent = rgbToOklch(accentRgb);
  const dark = relativeLuminance(canvasRgb) < 0.179;
  const hue = accent.C < 0.02 ? canvas.h : accent.h;
  const tintC = Math.min(0.045, Math.max(0.008, accent.C * 0.22));
  const step = dark ? 1 : -1;

  const surfaceAt = (deltaL: number, chroma = tintC): ThemeOklch => ({
    L: Math.min(0.98, Math.max(0.05, canvas.L + step * deltaL)),
    C: chroma,
    h: hue,
  });
  const toHex = (c: ThemeOklch) => rgbToHex(oklchToRgb(c));

  const textBase: ThemeOklch = {
    L: dark ? 0.95 : 0.2,
    C: Math.min(0.035, accent.C * 0.25),
    h: hue,
  };
  const text = solveOklchLightness(textBase, [canvasRgb], 7, dark ? "lighter" : "darker");
  const textRgb = oklchToRgb(text);
  const textMutedRgb = standardMutedThemeText(canvasRgb, textRgb);

  const action: ThemeOklch = {
    L: Math.min(0.85, Math.max(0.35, accent.L + (dark ? 0.06 : -0.02))),
    C: Math.max(accent.C * 0.9, 0.06),
    h: (hue + 50) % 360,
  };
  const actionRgb = oklchToRgb(action);
  const actionForeground = readableThemeForeground(actionRgb);
  const accentForeground = readableThemeForeground(accentRgb);

  const sidebar = surfaceAt(0.045, tintC * 1.4);
  const sidebarRgb = oklchToRgb(sidebar);
  const surface = surfaceAt(0.015);
  const surfaceRaised = surfaceAt(0.05);
  const surfaceRaisedRgb = oklchToRgb(surfaceRaised);
  const surfaceOverlay = surfaceAt(0.075);
  const border = surfaceAt(dark ? 0.16 : 0.12, Math.min(0.07, accent.C * 0.35));
  const input = surfaceAt(dark ? 0.21 : 0.16, Math.min(0.08, accent.C * 0.4));
  const secondary = surfaceAt(dark ? 0.1 : 0.06, Math.min(0.09, accent.C * 0.5));
  const secondaryRgb = oklchToRgb(secondary);
  const muted = surfaceAt(dark ? 0.06 : 0.04, Math.min(0.06, accent.C * 0.35));
  const mutedRgb = oklchToRgb(muted);
  const accentSurface = surfaceAt(dark ? 0.13 : 0.08, Math.min(0.11, accent.C * 0.55));
  const accentSurfaceRgb = oklchToRgb(accentSurface);
  const messageSurface = surfaceAt(dark ? 0.16 : 0.1, Math.min(0.13, accent.C * 0.6));
  const messageSurfaceRgb = oklchToRgb(messageSurface);
  const codeBackground = surfaceAt(0.035, tintC * 0.8);
  const updateSurface = surfaceAt(dark ? 0.14 : 0.09, Math.min(0.12, accent.C * 0.55));

  const foregroundOn = (surfaceRgb: ThemeRgbColor): string =>
    rgbToHex(
      oklchToRgb(solveOklchLightness(textBase, [surfaceRgb], 4.6, dark ? "lighter" : "darker")),
    );
  const mutedForeground = rgbToHex(readableThemeText(mutedRgb, textRgb, 1, 4.6));
  const placeholder = rgbToHex(readableThemeText(surfaceRaisedRgb, textRgb, 1, 4.6));

  const searchMatch = oklchToRgb(surfaceAt(0.22, Math.min(0.12, accent.C)));
  const searchMatchActive = oklchToRgb(surfaceAt(0.4, Math.min(0.18, accent.C)));
  const actionHover: ThemeOklch = { ...action, L: clamp(action.L + (dark ? 0.06 : -0.06), 0.05, 0.95) };

  return {
    ...defaults,
    ...standardStatusColors(canvasRgb),
    canvas: rgbToHex(canvasRgb),
    chrome: rgbToHex(canvasRgb),
    toolbar: rgbToHex(canvasRgb),
    toolbarForeground: rgbToHex(textRgb),
    toolbarBorder: toHex(surfaceAt(dark ? 0.14 : 0.1, Math.min(0.08, accent.C * 0.4))),
    toolbarControl: toHex(surfaceAt(dark ? 0.09 : 0.05, tintC * 1.3)),
    toolbarControlForeground: rgbToHex(textRgb),
    toolbarControlHover: toHex(surfaceAt(dark ? 0.14 : 0.09, tintC * 1.6)),
    surface: toHex(surface),
    surfaceRaised: toHex(surfaceRaised),
    surfaceOverlay: toHex(surfaceOverlay),
    text: rgbToHex(textRgb),
    textMuted: rgbToHex(textMutedRgb),
    border: toHex(border),
    input: toHex(input),
    focus: rgbToHex(accentRgb),
    accent: rgbToHex(accentRgb),
    accentForeground: rgbToHex(accentForeground),
    secondary: toHex(secondary),
    secondaryForeground: foregroundOn(secondaryRgb),
    muted: toHex(muted),
    mutedForeground,
    placeholder,
    secondaryLabel: rgbToHex(textMutedRgb),
    iconMuted: rgbToHex(textMutedRgb),
    update: rgbToHex(accentRgb),
    updateForeground: foregroundOn(oklchToRgb(updateSurface)),
    updateSurface: toHex(updateSurface),
    accentSurface: toHex(accentSurface),
    accentSurfaceForeground: foregroundOn(accentSurfaceRgb),
    messageSurface: toHex(messageSurface),
    messageForeground: foregroundOn(messageSurfaceRgb),
    messageAction: rgbToHex(actionRgb),
    messageActionForeground: rgbToHex(actionForeground),
    messageActionHover: toHex(actionHover),
    codeBackground: toHex(codeBackground),
    codeForeground: rgbToHex(textRgb),
    searchMatchBackground: rgbToHex(searchMatch),
    searchMatchForeground: rgbToHex(readableThemeForeground(searchMatch)),
    searchMatchActiveBackground: rgbToHex(searchMatchActive),
    searchMatchActiveForeground: rgbToHex(readableThemeForeground(searchMatchActive)),
    sidebar: toHex(sidebar),
    sidebarForeground: foregroundOn(sidebarRgb),
    sidebarMutedForeground: rgbToHex(standardMutedThemeText(sidebarRgb, textRgb)),
    sidebarControlSurface: toHex(surfaceAt(dark ? 0.1 : 0.07, tintC * 1.5)),
    sidebarRowHover: toHex(surfaceAt(dark ? 0.08 : 0.06, Math.min(0.08, accent.C * 0.45))),
    sidebarRowActive: toHex(surfaceAt(dark ? 0.12 : 0.09, Math.min(0.1, accent.C * 0.55))),
    sidebarRowSelected: toHex(surfaceAt(dark ? 0.14 : 0.1, Math.min(0.11, accent.C * 0.6))),
    sidebarBorder: toHex(surfaceAt(dark ? 0.17 : 0.12, Math.min(0.08, accent.C * 0.4))),
    terminalBackground: rgbToHex(canvasRgb),
    terminalForeground: rgbToHex(textRgb),
    terminalCursor: rgbToHex(accentRgb),
    terminalSelection: toHex(surfaceAt(dark ? 0.18 : 0.12, Math.min(0.12, accent.C * 0.55))),
    terminalScrollbar: toHex(surfaceAt(dark ? 0.22 : 0.16, tintC)),
    terminalScrollbarHover: toHex(surfaceAt(dark ? 0.3 : 0.22, tintC)),
  };
}

/** Maps a 61-role T3 Code ThemeColors object into Ferryx's allowlisted CSS token map. */
export function themeColorsToFerryxTokens(colors: ThemeColors): Record<string, string> {
  const hex = (v: string) => themeColorToHex(v) ?? v;
  const chatScreen = hex(colors.canvas);
  const chatSurface = hex(colors.surface);
  const chatForeground = readableThemeColorOn(
    readableThemeColorOn(hex(colors.text), chatScreen, 4.6),
    chatSurface,
    4.6,
  );
  const chatPrimary = hex(colors.accent);
  const chatPrimaryForeground = readableThemeColorOn(hex(colors.accentForeground), chatPrimary, 4.6);
  const chatLink = readableThemeColorOn(hex(colors.messageAction), chatScreen, 4.5);
  const chatCode = readableThemeColorOn(hex(colors.codeForeground), hex(colors.codeBackground), 4.5);
  const workingSolid = hex(colors.update);
  const workingContainer = hex(colors.updateSurface);
  const warningSolid = hex(colors.warning);
  const warningContainer = hex(colors.warningSurface);
  const errorSolid = hex(colors.error);
  const canvasRgb = parseColorToRgb(chatScreen) ?? { r: 10, g: 10, b: 10 };
  const dark = relativeLuminance(canvasRgb) < 0.179;
  const successSolid = dark ? "#86efac" : "#15803d";
  const successContainer = rgbToHex(mixRgb(canvasRgb, parseColorToRgb(successSolid)!, dark ? 0.16 : 0.1));
  const idleSolid = hex(colors.textMuted);
  const idleContainer = hex(colors.muted);

  const raw: Record<string, string> = {
    "--background": hex(colors.canvas),
    "--foreground": hex(colors.text),
    "--card": hex(colors.surface),
    "--panel": hex(colors.surfaceRaised),
    "--card-foreground": hex(colors.text),
    "--popover": hex(colors.surfaceOverlay),
    "--popover-foreground": hex(colors.text),
    "--primary": chatPrimary,
    "--primary-foreground": chatPrimaryForeground,
    "--secondary": hex(colors.secondary),
    "--secondary-foreground": hex(colors.secondaryForeground),
    "--muted": hex(colors.muted),
    "--muted-foreground": hex(colors.mutedForeground),
    "--accent": hex(colors.accentSurface),
    "--accent-foreground": hex(colors.accentSurfaceForeground),
    "--surface-hover": hex(colors.toolbarControlHover),
    "--destructive": errorSolid,
    "--destructive-foreground": readableThemeColorOn(hex(colors.errorForeground), errorSolid, 4.5),
    "--border": hex(colors.border),
    "--input": hex(colors.input),
    "--ring": hex(colors.focus),
    "--sidebar": hex(colors.sidebar),
    "--sidebar-foreground": hex(colors.sidebarForeground),
    "--sidebar-primary": chatPrimary,
    "--sidebar-primary-foreground": chatPrimaryForeground,
    "--sidebar-accent": hex(colors.sidebarRowActive),
    "--sidebar-accent-foreground": hex(colors.sidebarForeground),
    "--sidebar-border": hex(colors.sidebarBorder),
    "--sidebar-ring": hex(colors.focus),
    "--worktree-sidebar": hex(colors.sidebar),
    "--worktree-sidebar-foreground": hex(colors.sidebarForeground),
    "--worktree-sidebar-accent": hex(colors.sidebarRowSelected),
    "--worktree-sidebar-accent-foreground": hex(colors.sidebarForeground),
    "--worktree-sidebar-border": hex(colors.sidebarBorder),
    "--worktree-sidebar-ring": hex(colors.focus),
    "--terminal": hex(colors.terminalBackground),
    "--terminal-divider": hex(colors.border),
    "--status-working": workingSolid,
    "--status-working-solid": workingSolid,
    "--status-working-container": workingContainer,
    "--status-working-border": workingSolid,
    "--status-working-on-container": readableThemeColorOn(hex(colors.updateForeground), workingContainer, 4.6),
    "--status-warning": warningSolid,
    "--status-warning-solid": warningSolid,
    "--status-warning-container": warningContainer,
    "--status-warning-border": warningSolid,
    "--status-warning-on-container": readableThemeColorOn(hex(colors.warningForeground), warningContainer, 4.6),
    "--status-success": successSolid,
    "--status-success-solid": successSolid,
    "--status-success-container": successContainer,
    "--status-success-border": successSolid,
    "--status-success-on-container": readableThemeColorOn(successSolid, successContainer, 4.6),
    "--status-idle": idleSolid,
    "--status-idle-solid": idleSolid,
    "--status-idle-container": idleContainer,
    "--status-idle-border": idleSolid,
    "--status-idle-on-container": readableThemeColorOn(idleSolid, idleContainer, 4.6),
    "--chat-screen": chatScreen,
    "--chat-surface": chatSurface,
    "--chat-surface-raised": hex(colors.surfaceRaised),
    "--chat-surface-hover": hex(colors.sidebarRowHover),
    "--chat-user-bubble": hex(colors.messageSurface),
    "--chat-foreground": chatForeground,
    "--chat-foreground-secondary": hex(colors.textMuted),
    "--chat-foreground-tertiary": hex(colors.placeholder),
    "--chat-border": hex(colors.border),
    "--chat-primary": chatPrimary,
    "--chat-primary-foreground": chatPrimaryForeground,
    "--chat-link": chatLink,
    "--chat-code": chatCode,
    "--chat-danger": errorSolid,
    "--chat-row-hover": hex(colors.sidebarRowHover),
    "--chat-composer-panel": hex(colors.canvas),
    "--chat-composer-surface": hex(colors.surfaceRaised),
    "--chat-composer-border": hex(colors.input),
    "--chat-header": hex(colors.toolbar),
    "--chat-header-border": hex(colors.toolbarBorder),
  };

  const filtered: Record<string, string> = {};
  for (const [key, value] of Object.entries(raw)) {
    if (THEME_TOKEN_ALLOWLIST.has(key) && parseThemeColor(value)) {
      filtered[key] = value;
    }
  }
  return filtered;
}

export type TerminalThemePalette = Readonly<{
  background: string;
  foreground: string;
  cursor: string;
  selectionBackground: string;
}>;

export function themeColorsToTerminalPalette(colors: ThemeColors): TerminalThemePalette {
  return {
    background: themeColorToHex(colors.terminalBackground) ?? "#282c34",
    foreground: themeColorToHex(colors.terminalForeground) ?? "#fafafa",
    cursor: themeColorToHex(colors.terminalCursor) ?? "#e5e5e5",
    selectionBackground: themeColorToHex(colors.terminalSelection) ?? "#40454f",
  };
}

export function themeIdFromName(name: string): string {
  const slug = name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48);
  return slug.length >= 2 ? slug : `theme-${slug || "custom"}`;
}

export function isValidThemeId(id: unknown): id is string {
  return typeof id === "string" && /^[a-z0-9][a-z0-9-]{1,63}$/.test(id);
}

/** Converts a curated or imported ThemeDefinition into a Ferryx ThemeDocumentV1. */
export function themeDefinitionToDocument(
  def: ThemeDefinition,
  seed?: { canvas: string; accent: string },
): ThemeDocumentV1 {
  const baseTokens = themeColorsToFerryxTokens(def.colors);
  const variants: Partial<Record<ThemeAppearance, Record<string, string>>> = {};
  const variantRoles: Partial<Record<ThemeAppearance, Partial<Record<ThemeColorRole, string>>>> = {};
  if (def.variants) {
    for (const mode of ["light", "dark"] as const) {
      const variantColors = def.variants[mode];
      if (variantColors && mode !== def.appearance) {
        variants[mode] = themeColorsToFerryxTokens(variantColors);
        variantRoles[mode] = { ...variantColors };
      }
    }
  }
  return {
    version: 1,
    id: def.id,
    name: def.label,
    base: def.appearance,
    tokens: baseTokens,
    roles: { ...def.colors },
    ...(Object.keys(variants).length > 0 ? { variants, variantRoles } : {}),
    ...(def.collection ? { collection: def.collection } : {}),
    ...(seed ? { seed } : {}),
    ...(def.css ? { css: def.css, cssScope: "app" as const } : {}),
  };
}

/** Resolves the effective ThemeColors for a ThemeDocumentV1 in a target appearance. */
export function resolveDocumentRoles(
  doc: ThemeDocumentV1,
  targetAppearance?: ThemeAppearance,
): ThemeColors {
  const appearance = targetAppearance ?? doc.base;
  const fallback = getStandardThemeColors(appearance);
  const rawRoles =
    appearance !== doc.base && doc.variantRoles?.[appearance]
      ? doc.variantRoles[appearance]
      : doc.roles;
  if (rawRoles) {
    return { ...fallback, ...rawRoles } as ThemeColors;
  }
  const activeTokens =
    appearance !== doc.base && doc.variants?.[appearance]
      ? doc.variants[appearance]!
      : doc.tokens;
  const canvas = activeTokens["--background"] ?? activeTokens["--chat-screen"];
  const accent = activeTokens["--primary"] ?? activeTokens["--chat-primary"];
  if (canvas && accent) {
    const derived = createVividThemeColors(appearance, canvas, accent);
    return {
      ...derived,
      ...(activeTokens["--foreground"] ? { text: activeTokens["--foreground"] } : {}),
      ...(activeTokens["--card"] ? { surface: activeTokens["--card"] } : {}),
      ...(activeTokens["--panel"] ? { surfaceRaised: activeTokens["--panel"] } : {}),
      ...(activeTokens["--popover"] ? { surfaceOverlay: activeTokens["--popover"] } : {}),
      ...(activeTokens["--sidebar"] ? { sidebar: activeTokens["--sidebar"] } : {}),
      ...(activeTokens["--terminal"] ? { terminalBackground: activeTokens["--terminal"] } : {}),
      ...(activeTokens["--border"] ? { border: activeTokens["--border"] } : {}),
    };
  }
  return fallback;
}

export type T3ThemeParseResult =
  | { ok: true; doc: ThemeDocumentV1; definition: ThemeDefinition }
  | { ok: false; errors: Array<{ path: string; message: string }> };

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseRoleOverrides(
  raw: unknown,
  pathPrefix: string,
  errors: Array<{ path: string; message: string }>,
): Partial<Record<ThemeColorRole, string>> {
  if (!isRecord(raw)) {
    errors.push({ path: pathPrefix, message: "Expected a colors object." });
    return {};
  }
  const result: Partial<Record<ThemeColorRole, string>> = {};
  for (const [key, value] of Object.entries(raw)) {
    if (!THEME_COLOR_ROLE_SET.has(key)) continue; // tolerate forward-compatible T3 roles gracefully
    const canonical = toCanonicalThemeColor(value);
    if (!canonical) {
      errors.push({ path: `${pathPrefix}.${key}`, message: `Invalid color value for ${key}.` });
    } else {
      result[key as ThemeColorRole] = canonical;
    }
  }
  return result;
}

/**
 * Parses T3 Code full v1 JSON ({ version: 1, id?, name, appearance, colors, variants?, css? })
 * AND T3 Code shorthand JSON ({ name, appearance, canvas, accent, colors?, variants?, css? }).
 */
export function parseT3ThemeFile(raw: unknown): T3ThemeParseResult {
  const errors: Array<{ path: string; message: string }> = [];
  if (!isRecord(raw)) {
    return { ok: false, errors: [{ path: "$", message: "Theme files must contain a JSON object." }] };
  }
  const hasVersion = raw.version !== undefined;
  const isShorthand = !hasVersion && typeof raw.canvas === "string" && typeof raw.accent === "string";
  if (hasVersion && raw.version !== THEME_FILE_VERSION) {
    errors.push({ path: "version", message: `Expected theme file version ${THEME_FILE_VERSION}.` });
  }
  const name = typeof raw.name === "string" ? raw.name.trim() : "";
  if (!name || name.length > 64) {
    errors.push({ path: "name", message: "Theme files need a name (1-64 characters)." });
  }
  const appearance = raw.appearance;
  if (appearance !== "light" && appearance !== "dark") {
    errors.push({ path: "appearance", message: 'Theme files need an appearance of "light" or "dark".' });
  }
  const id = raw.id === undefined ? (name ? themeIdFromName(name) : "") : raw.id;
  if (!isValidThemeId(id)) {
    errors.push({ path: "id", message: "Theme ids may only contain 2-64 lowercase letters, numbers, and hyphens." });
  } else if (RESERVED_THEME_IDS.has(id)) {
    errors.push({ path: "id", message: `The theme id "${id}" is reserved.` });
  }
  if (!isShorthand && raw.colors === undefined) {
    errors.push({ path: "colors", message: "Theme files need a colors object (or canvas + accent seed colors)." });
  }
  if (raw.css !== undefined && (typeof raw.css !== "string" || raw.css.length > THEME_CSS_MAX_LENGTH)) {
    errors.push({ path: "css", message: `Theme CSS must be text of ${THEME_CSS_MAX_LENGTH} characters or fewer.` });
  }
  let seed: { canvas: string; accent: string } | undefined;
  if (raw.canvas !== undefined || raw.accent !== undefined) {
    const canvas = toCanonicalThemeColor(raw.canvas);
    const accent = toCanonicalThemeColor(raw.accent);
    if (!canvas) errors.push({ path: "canvas", message: "Invalid canvas seed color." });
    if (!accent) errors.push({ path: "accent", message: "Invalid accent seed color." });
    if (canvas && accent) seed = { canvas, accent };
  }
  const overrides = raw.colors !== undefined ? parseRoleOverrides(raw.colors, "colors", errors) : {};
  const validAppearance: ThemeAppearance = appearance === "light" ? "light" : "dark";
  const baseColors = seed
    ? createVividThemeColors(validAppearance, seed.canvas, seed.accent)
    : getStandardThemeColors(validAppearance);
  const resolvedColors: ThemeColors = { ...baseColors, ...overrides };

  const variants: Partial<Record<ThemeAppearance, ThemeColors>> = {};
  if (raw.variants !== undefined) {
    if (!isRecord(raw.variants)) {
      errors.push({ path: "variants", message: "Theme variants must be an object." });
    } else {
      for (const [mode, vColors] of Object.entries(raw.variants)) {
        if (mode !== "light" && mode !== "dark") {
          errors.push({ path: `variants.${mode}`, message: 'Theme variants may only be "light" or "dark".' });
        } else if (mode === appearance) {
          errors.push({ path: `variants.${mode}`, message: `Variant must not repeat base appearance "${mode}".` });
        } else {
          const vOverrides = parseRoleOverrides(vColors, `variants.${mode}`, errors);
          const vBase =
            vOverrides.canvas && vOverrides.accent
              ? createVividThemeColors(mode, vOverrides.canvas, vOverrides.accent)
              : getStandardThemeColors(mode);
          variants[mode] = { ...vBase, ...vOverrides };
        }
      }
    }
  }

  let collection: ThemeCollection | undefined;
  if (raw.collection !== undefined) {
    if (isRecord(raw.collection) && typeof raw.collection.id === "string" && typeof raw.collection.label === "string") {
      collection = { id: raw.collection.id, label: raw.collection.label };
    } else {
      errors.push({ path: "collection", message: "Theme collection requires id and label strings." });
    }
  }

  if (errors.length > 0) return { ok: false, errors };
  const definition: ThemeDefinition = {
    id: id as string,
    label: name,
    appearance: validAppearance,
    colors: resolvedColors,
    ...(Object.keys(variants).length > 0 ? { variants } : {}),
    ...(collection ? { collection } : {}),
    ...(typeof raw.css === "string" && raw.css.length > 0 ? { css: raw.css } : {}),
  };
  return {
    ok: true,
    definition,
    doc: themeDefinitionToDocument(definition, seed),
  };
}

/** Exports any ThemeDocumentV1 in official T3 Code v1 JSON format. */
export function serializeToT3ThemeFile(doc: ThemeDocumentV1): string {
  const colors = resolveDocumentRoles(doc, doc.base);
  const other: ThemeAppearance = doc.base === "light" ? "dark" : "light";
  const hasOther = Boolean(doc.variantRoles?.[other] || doc.variants?.[other]);
  const payload: Record<string, unknown> = {
    version: THEME_FILE_VERSION,
    id: doc.id,
    name: doc.name,
    appearance: doc.base,
    colors,
    ...(hasOther ? { variants: { [other]: resolveDocumentRoles(doc, other) } } : {}),
    ...(doc.collection ? { collection: doc.collection } : {}),
    ...(doc.css ? { css: doc.css } : {}),
  };
  return JSON.stringify(payload, null, 2);
}

/** T3 Code preview circle gradient specification (packages/shared/src/themePreview.ts parity). */
export type ThemePreviewColors = Readonly<{
  canvas: string;
  accent: string;
  messageAction: string;
}>;

export function previewColorsOf(
  colors: ThemeColors | Record<string, string>,
  appearance: ThemeAppearance,
): ThemePreviewColors {
  if ("canvas" in colors && typeof colors.canvas === "string") {
    const c = colors as ThemeColors;
    return {
      canvas: themeColorToHex(c.canvas) ?? (appearance === "light" ? "#fcfcfc" : "#0a0a0a"),
      accent: themeColorToHex(c.accent) ?? (appearance === "light" ? "#f4f4f5" : "#1c1c1f"),
      messageAction: themeColorToHex(c.messageAction) ?? (appearance === "light" ? "#4f46e5" : "#8b9cff"),
    };
  }
  const t = colors as Record<string, string>;
  return {
    canvas: themeColorToHex(t["--background"] ?? t["--chat-screen"] ?? "") ?? (appearance === "light" ? "#fcfcfc" : "#0a0a0a"),
    accent: themeColorToHex(t["--primary"] ?? t["--chat-primary"] ?? "") ?? (appearance === "light" ? "#2563eb" : "#346bf1"),
    messageAction: themeColorToHex(t["--chat-link"] ?? t["--ring"] ?? t["--primary"] ?? "") ?? (appearance === "light" ? "#4f46e5" : "#8b9cff"),
  };
}

export function buildThemePreviewGradient(preview: ThemePreviewColors, appearance: ThemeAppearance): string {
  const baseRgb = parseColorToRgb(preview.canvas) ?? (appearance === "light" ? { r: 252, g: 252, b: 252 } : { r: 10, g: 10, b: 10 });
  const target = appearance === "light" ? { r: 255, g: 255, b: 255 } : { r: 9, g: 9, b: 11 };
  const base = rgbToHex(mixRgb(target, baseRgb, 0.8));
  const [ax, ay] = appearance === "light" ? ["72%", "22%"] : ["28%", "78%"];
  const [mx, my] = appearance === "light" ? ["18%", "82%"] : ["82%", "18%"];
  return [
    `radial-gradient(circle at ${ax} ${ay}, ${preview.accent} 0%, transparent 58%)`,
    `radial-gradient(circle at ${mx} ${my}, ${preview.messageAction} 0%, transparent 55%)`,
    base,
  ].join(", ");
}

/** Organized role groups for the Visual Theme Editor & Element Inspector. */
export const THEME_ROLE_GROUPS: ReadonlyArray<{
  id: string;
  label: string;
  roles: ReadonlyArray<{ role: ThemeColorRole; label: string; ferryxToken?: string }>;
}> = [
  {
    id: "surfaces",
    label: "Surfaces & Chrome",
    roles: [
      { role: "canvas", label: "Canvas", ferryxToken: "--background" },
      { role: "surface", label: "Surface / Card", ferryxToken: "--card" },
      { role: "surfaceRaised", label: "Raised Panel", ferryxToken: "--panel" },
      { role: "surfaceOverlay", label: "Popover / Dialog", ferryxToken: "--popover" },
      { role: "toolbar", label: "Header / Toolbar", ferryxToken: "--chat-header" },
      { role: "toolbarControlHover", label: "Surface Hover", ferryxToken: "--surface-hover" },
    ],
  },
  {
    id: "text-borders",
    label: "Text & Borders",
    roles: [
      { role: "text", label: "Primary Text", ferryxToken: "--foreground" },
      { role: "textMuted", label: "Muted Text", ferryxToken: "--muted-foreground" },
      { role: "border", label: "Border", ferryxToken: "--border" },
      { role: "input", label: "Input Border", ferryxToken: "--input" },
      { role: "focus", label: "Focus Ring", ferryxToken: "--ring" },
    ],
  },
  {
    id: "brand-status",
    label: "Brand & Status",
    roles: [
      { role: "accent", label: "Accent / Primary", ferryxToken: "--primary" },
      { role: "accentForeground", label: "On Accent", ferryxToken: "--primary-foreground" },
      { role: "accentSurface", label: "Accent Surface", ferryxToken: "--accent" },
      { role: "messageAction", label: "Link / Action", ferryxToken: "--chat-link" },
      { role: "error", label: "Error / Destructive", ferryxToken: "--destructive" },
      { role: "warning", label: "Warning", ferryxToken: "--status-warning" },
    ],
  },
  {
    id: "sidebar",
    label: "Sidebar",
    roles: [
      { role: "sidebar", label: "Sidebar Background", ferryxToken: "--sidebar" },
      { role: "sidebarForeground", label: "Sidebar Text", ferryxToken: "--sidebar-foreground" },
      { role: "sidebarRowActive", label: "Active Row", ferryxToken: "--sidebar-accent" },
      { role: "sidebarRowSelected", label: "Selected Worktree", ferryxToken: "--worktree-sidebar-accent" },
      { role: "sidebarBorder", label: "Sidebar Border", ferryxToken: "--sidebar-border" },
    ],
  },
  {
    id: "chat-code",
    label: "Chat & Code",
    roles: [
      { role: "messageSurface", label: "User Bubble", ferryxToken: "--chat-user-bubble" },
      { role: "codeBackground", label: "Code Background", ferryxToken: "--chat-surface-raised" },
      { role: "codeForeground", label: "Code Text", ferryxToken: "--chat-code" },
      { role: "secondary", label: "Secondary Control", ferryxToken: "--secondary" },
      { role: "muted", label: "Muted Surface", ferryxToken: "--muted" },
    ],
  },
  {
    id: "terminal",
    label: "Terminal",
    roles: [
      { role: "terminalBackground", label: "Terminal Background", ferryxToken: "--terminal" },
      { role: "terminalForeground", label: "Terminal Foreground" },
      { role: "terminalCursor", label: "Terminal Cursor" },
      { role: "terminalSelection", label: "Terminal Selection" },
    ],
  },
];

const ROLE_LABEL_MAP = new Map<ThemeColorRole, string>(
  THEME_ROLE_GROUPS.flatMap((g) => g.roles.map((r) => [r.role, r.label] as const)),
);

export function getThemeRoleLabel(role: ThemeColorRole): string {
  return ROLE_LABEL_MAP.get(role) ?? role.replace(/([A-Z])/g, " $1").replace(/^./, (c) => c.toUpperCase());
}
