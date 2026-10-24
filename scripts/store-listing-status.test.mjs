import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";

import { classifyListing, extractTitle, fetchListingStatus, STORE_DETAIL_URL } from "./store-listing-status.mjs";

const FIXTURES = join(import.meta.dir, "fixtures", "store-listing");
const live = readFileSync(join(FIXTURES, "live.html"), "utf8");
const notFound = readFileSync(join(FIXTURES, "not-found.html"), "utf8");
// Observed 2026-10-24: the unpublished product redirects here with HTTP 410.
const NOT_FOUND_URL =
  "https://apps.microsoft.com/404/product?errorCode=ProductNotFound&id=9nlhql5jnlm4&name=&redirectedFrom=%2Fdetail%2F9nlhql5jnlm4";

test("live fixture page classifies as live from the product title", () => {
  const result = classifyListing({ html: live, finalUrl: `${STORE_DETAIL_URL}?hl=en-US`, status: 200 });
  expect(result.live).toBe(true);
  expect(result.title).toBe("Ferryx - Free download and install on Windows | Microsoft Store");
});

test("not-found fixture page classifies as not live", () => {
  const result = classifyListing({ html: notFound, finalUrl: NOT_FOUND_URL, status: 410 });
  expect(result.live).toBe(false);
  expect(result.reason).toBe("product-not-found");
});

test("generic Store title without a redirect is still not live", () => {
  const result = classifyListing({ html: notFound, finalUrl: STORE_DETAIL_URL, status: 200 });
  expect(result.live).toBe(false);
  expect(result.reason).toBe("title-does-not-name-product");
  expect(extractTitle(notFound)).toBe("Microsoft Store - Download apps, games & more for your Windows PC");
});

test("fetchListingStatus classifies the response returned by fetch", async () => {
  const fakeFetch = async () => {
    const response = new Response(notFound, { status: 410 });
    Object.defineProperty(response, "url", { value: NOT_FOUND_URL });
    return response;
  };
  const result = await fetchListingStatus({ fetchImpl: fakeFetch });
  expect(result.live).toBe(false);
  expect(result.finalUrl).toBe(NOT_FOUND_URL);
});
