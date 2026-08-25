/**
 * Detect whether a string looks like a complete cURL command rather than a URL.
 *
 * Used by the URL bar to decide whether to offer "Parse as cURL" instead of
 * treating the input as a literal URL. The check is intentionally generous
 * (curl + flag, or leading `curl` keyword, or any token starting with `-` that
 * is not a URL like `-H`, `-d`, `-X`, `-u`, `--url`, `--data`).
 *
 * Heuristics, in priority order:
 *  1. Starts with `curl` (after optional whitespace / line-continuation backslashes)
 *  2. Contains a recognised curl flag token (`-H`, `-d`, `--request`, etc.)
 *  3. Falls back to false for plain URLs.
 */
export function looksLikeCurl(input: string): boolean {
  if (!input) return false;

  // First non-empty line: leading char should be `curl`, possibly preceded by
  // shell escapes like `$ ` or `>`.
  const trimmedStart = input.trimStart();
  if (/^(curl)\b/.test(trimmedStart)) return true;

  // Look for a curl flag anywhere in the text — common flags a user pastes
  // from DevTools / Postman / docs:
  //   -H / --header
  //   -d / --data / --data-raw / --data-binary
  //   -X / --request
  //   -u / --user
  //   -k / --insecure
  //   -L / --location
  //   --url (postman-style)
  if (/(^|\s)(--url|-H|--header|-d|--data(?:-raw|-binary)?|-X|--request|-u|--user|-k|--insecure|-L|--location)\b/.test(input)) {
    return true;
  }

  return false;
}
