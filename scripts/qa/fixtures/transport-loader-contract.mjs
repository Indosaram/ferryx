/** Import-only fixture: deliberately lacks real transport provisioning. */
export function loadTransportFixture(ctx) {
  ctx.transportLoaderCalls = [...(ctx.transportLoaderCalls ?? []), ctx.scenario];
  return { loaderContract: 'local-module-imported' };
}
