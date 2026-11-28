// Registrable-domain shortening: the label Maho's pills and Tidy folders show for a site.
//
// These cases pin the two things that matter to the UI: a public suffix never becomes the site
// (`co.uk` is not a domain), and an unknown suffix never merges two unrelated sites.

import { describe, expect, it } from "vitest";

import {
  MULTI_PART_SUFFIXES,
  hostFromUrl,
  registrableDomain,
  shortHost,
} from "./registrableDomain";

describe("registrableDomain", () => {
  it("keeps the label in front of a single-label TLD", () => {
    expect(registrableDomain("github.com")).toBe("github.com");
    expect(registrableDomain("docs.rs")).toBe("docs.rs");
    expect(registrableDomain("example.io")).toBe("example.io");
  });

  it("strips subdomains down to the owning label", () => {
    expect(registrableDomain("docs.example.com")).toBe("example.com");
    expect(registrableDomain("a.b.c.example.com")).toBe("example.com");
    expect(registrableDomain("developer.mozilla.org")).toBe("mozilla.org");
  });

  it("strips www. before shortening", () => {
    expect(registrableDomain("www.example.com")).toBe("example.com");
    expect(registrableDomain("WWW.Example.COM")).toBe("example.com");
  });

  it("keeps a two-part public suffix intact", () => {
    expect(registrableDomain("example.co.uk")).toBe("example.co.uk");
    expect(registrableDomain("shop.example.co.uk")).toBe("example.co.uk");
    expect(registrableDomain("www.bbc.co.uk")).toBe("bbc.co.uk");
    expect(registrableDomain("example.com.au")).toBe("example.com.au");
    expect(registrableDomain("a.example.com.au")).toBe("example.com.au");
    expect(registrableDomain("example.co.jp")).toBe("example.co.jp");
    expect(registrableDomain("example.or.kr")).toBe("example.or.kr");
    expect(registrableDomain("example.ne.jp")).toBe("example.ne.jp");
  });

  it("treats a hosting platform subdomain as its own site", () => {
    expect(registrableDomain("ferryx.github.io")).toBe("ferryx.github.io");
    expect(registrableDomain("app.ferryx.github.io")).toBe("ferryx.github.io");
    expect(registrableDomain("maho.vercel.app")).toBe("maho.vercel.app");
    expect(registrableDomain("docs.pages.dev")).toBe("docs.pages.dev");
    expect(registrableDomain("site.netlify.app")).toBe("site.netlify.app");
    expect(registrableDomain("api.herokuapp.com")).toBe("api.herokuapp.com");
  });

  it("returns a bare host that has no registrable part unchanged", () => {
    expect(registrableDomain("localhost")).toBe("localhost");
    expect(registrableDomain("intranet")).toBe("intranet");
    expect(registrableDomain("127.0.0.1")).toBe("127.0.0.1");
    expect(registrableDomain("::1")).toBe("::1");
  });

  it("normalizes case, surrounding dots and empty input", () => {
    expect(registrableDomain("  GitHub.COM. ")).toBe("github.com");
    expect(registrableDomain("")).toBe("");
    expect(registrableDomain("...")).toBe("");
  });

  it("never returns more than suffix plus one label", () => {
    for (const suffix of MULTI_PART_SUFFIXES) {
      const domain = registrableDomain(`deep.sub.example.${suffix}`);
      expect(domain).toBe(`example.${suffix}`);
      expect(domain.split(".").length).toBe(suffix.split(".").length + 1);
    }
  });
});

describe("hostFromUrl", () => {
  it("reads the bare host, dropping port, userinfo and a trailing dot", () => {
    expect(hostFromUrl("https://user:pw@www.example.com:8443/docs?a=1#x")).toBe("www.example.com");
    expect(hostFromUrl("https://example.com./path")).toBe("example.com");
  });

  it("has no host for a non-URL, a new tab or a file", () => {
    expect(hostFromUrl("")).toBeNull();
    expect(hostFromUrl("about:blank")).toBeNull();
    expect(hostFromUrl("file:///tmp/page.html")).toBeNull();
    expect(hostFromUrl("not a url")).toBeNull();
  });
});

describe("shortHost", () => {
  it("shortens a page URL to its registrable domain", () => {
    expect(shortHost("https://www.tauri.app/start/")).toBe("tauri.app");
    expect(shortHost("https://docs.example.co.uk/a/b")).toBe("example.co.uk");
    expect(shortHost("https://ferryx.github.io/ferryx/")).toBe("ferryx.github.io");
  });

  it("falls back for a URL with no host", () => {
    expect(shortHost("about:blank", "New Tab")).toBe("New Tab");
    expect(shortHost("junk", "New Tab")).toBe("New Tab");
    expect(shortHost("about:blank")).toBe("");
  });
});
