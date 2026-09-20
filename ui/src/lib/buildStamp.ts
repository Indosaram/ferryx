/// Build identity injected by vite.config.ts. Test harnesses and any build without the define see
/// "dev" so the stamp can never break a render.
export const BUILD_STAMP: string = typeof __FERRYX_BUILD__ === "string" ? __FERRYX_BUILD__ : "dev";
