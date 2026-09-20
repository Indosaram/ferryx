export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.protocol !== 'https:' || url.hostname === 'www.ferryx.dev') {
      url.protocol = 'https:';
      url.hostname = 'ferryx.dev';
      return Response.redirect(url.toString(), 301);
    }
    return env.ASSETS.fetch(request);
  },
};
