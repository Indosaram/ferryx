#!/usr/bin/env bun
// Reports whether the Ferryx Microsoft Store listing is publicly live.
// Live = apps.microsoft.com serves the product detail page whose <title> names the product
// ("Ferryx - ... | Microsoft Store"). Not live = the Store's 404/ProductNotFound page (served
// with the generic "Microsoft Store - Download apps..." title) or any other title.
// Certification acceptance in Partner Center is not publication; only this public page counts.
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const STORE_PRODUCT_ID = "9NLHQL5JNLM4";
export const STORE_DETAIL_URL = `https://apps.microsoft.com/detail/${STORE_PRODUCT_ID}`;
export const PRODUCT_NAME = "Ferryx";

function decodeEntities(text) {
  return text
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
}

export function extractTitle(html) {
  const match = html.match(/<title[^>]*>([\s\S]*?)<\/title>/i);
  return match ? decodeEntities(match[1]).trim() : null;
}

/**
 * Classifies a fetched Store page. `finalUrl` is the post-redirect URL; the Store redirects
 * unknown or unpublished products to /404/product?errorCode=ProductNotFound.
 */
export function classifyListing({ html, finalUrl = "", status = 200, productName = PRODUCT_NAME }) {
  const title = extractTitle(html ?? "");
  if (/\/404\/|ProductNotFound/i.test(finalUrl)) {
    return { live: false, reason: "product-not-found", title, status };
  }
  if (status < 200 || status >= 300) {
    return { live: false, reason: `http-${status}`, title, status };
  }
  const titleNamesProduct =
    title !== null && title.toLowerCase().startsWith(productName.toLowerCase()) && /Microsoft Store/i.test(title);
  if (!titleNamesProduct) {
    return { live: false, reason: "title-does-not-name-product", title, status };
  }
  return { live: true, reason: "product-title", title, status };
}

export async function fetchListingStatus({ url = STORE_DETAIL_URL, fetchImpl = fetch } = {}) {
  const response = await fetchImpl(url, {
    redirect: "follow",
    headers: { "user-agent": "Mozilla/5.0 (ferryx store-listing-status)", "accept-language": "en-US" },
  });
  const html = await response.text();
  return { url, finalUrl: response.url || url, ...classifyListing({ html, finalUrl: response.url || url, status: response.status }) };
}

async function main() {
  const result = await fetchListingStatus();
  const checkedAt = new Date().toISOString();
  console.log(JSON.stringify({ checkedAt, productId: STORE_PRODUCT_ID, ...result }, null, 2));
  console.log(result.live ? `live: ${STORE_DETAIL_URL}` : `not-live: ${result.reason}`);
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))) {
  main().catch((err) => {
    console.error(`store-listing-status: ${err.message}`);
    process.exit(2);
  });
}
