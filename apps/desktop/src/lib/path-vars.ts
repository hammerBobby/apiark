/**
 * Extract `:paramName` path variables from a URL, ignoring any `:port`
 * segment of the host.
 *
 * Used by both the live UI panel and the file-storage layer so they agree
 * on what counts as a "path variable".
 *
 * @example
 *   extractPathVarsFromUrl("http://host:31003/api/users/:userId")
 *     // => ["userId"]
 *   extractPathVarsFromUrl("https://h.example.com/foo")
 *     // => []
 *   extractPathVarsFromUrl("/orders/:orderId/items/:itemId?lang=zh")
 *     // => ["orderId", "itemId"]
 */
export function extractPathVarsFromUrl(url: string): string[] {
  if (!url) return [];

  // Locate the start of the path part. We never scan scheme + host + port.
  let pathStart = 0;
  const schemeMatch = url.match(/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//);
  if (schemeMatch) {
    const afterScheme = schemeMatch[0].length;
    const hostEnd = url.indexOf("/", afterScheme);
    if (hostEnd === -1) return []; // no path
    pathStart = hostEnd;
  } else if (url.startsWith("/")) {
    pathStart = 0;
  } else if (/^[^/]*$/.test(url)) {
    // Bare hostname, no scheme, no '/'.
    return [];
  }

  // Match `:name` where `name` is letters/digits/underscore (port-style
  // matches like `:31003` are purely digits and dropped below).
  const matches = url.slice(pathStart).match(/:([A-Za-z_][\w]*|[\d]+)/g);
  if (!matches) return [];

  const seen = new Set<string>();
  const out: string[] = [];
  for (const m of matches) {
    const name = m.slice(1);
    if (/^\d+$/.test(name)) continue; // skip `:port`-like patterns
    if (!seen.has(name)) {
      seen.add(name);
      out.push(name);
    }
  }
  return out;
}
