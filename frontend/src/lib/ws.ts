/**
 * Convert an HTTP(S) endpoint or relative path to an absolute WebSocket URL.
 * Relative paths are resolved against the current origin; some older browsers
 * (e.g. Chrome < 126, still common on Windows 10) reject relative URLs in the
 * WebSocket constructor.
 */
export function toWsUrl(endpoint: string): string {
  if (/^https?:/.test(endpoint)) {
    return endpoint.replace(/^http/, 'ws');
  }
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}${endpoint}`;
}
