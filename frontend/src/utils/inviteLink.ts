import * as Linking from 'expo-linking';

/**
 * `Linking.createURL` defaults to a *two*-slash URL (`shelfcircle://invite/...`)
 * — `isTripleSlashed` must be passed explicitly to get a triple-slash one
 * (`shelfcircle:///invite/...`). This matters: without it, WHATWG URL parsing
 * (which `Linking.parse` and the standard `URL` constructor both do) treats
 * `invite` as the *hostname*, not part of the path — `new
 * URL('shelfcircle://invite/abc123').pathname` is `/abc123`, not
 * `/invite/abc123` — so `parseInviteToken` below could never recover the
 * token from the app's own generated links. Verified against
 * `node_modules/expo-linking`'s actual source and a plain `new URL(...)`
 * trace, not just the docs.
 */
export function buildInviteUrl(token: string): string {
  return Linking.createURL(`invite/${token}`, { isTripleSlashed: true });
}

/** Extracts the invite token from a scanned QR value or opened deep link, or
 * `null` if it isn't one of ours. */
export function parseInviteToken(url: string): string | null {
  let path: string | null;
  try {
    path = Linking.parse(url).path;
  } catch {
    return null;
  }
  if (!path) return null;
  const match = /^invite\/(.+)$/.exec(path);
  return match ? match[1] : null;
}
