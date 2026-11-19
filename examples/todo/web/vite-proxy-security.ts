/** Reject a cross-site browser request before dev proxy Origin translation.
 * The production Rust host never relies on this development-only adapter.
 */
export function trustedDevOrigin(headers: {
  host?: string;
  origin?: string;
  'sec-fetch-site'?: string | string[];
}): boolean {
  const { host, origin } = headers;
  const fetchSite = headers['sec-fetch-site'];

  return (
    typeof host === 'string' &&
    /^127\.0\.0\.1:[0-9]+$/.test(host) &&
    origin === 'http://' + host &&
    (fetchSite === undefined || fetchSite === 'same-origin' || fetchSite === 'none')
  );
}
