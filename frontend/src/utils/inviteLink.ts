import * as Linking from 'expo-linking';

/**
 * `Linking.createURL` (rather than hand-building `shelfcircle://...`) keeps
 * the host segment empty (`shelfcircle:///invite/...`), which is what makes
 * `Linking.parse` reliably return the whole thing as `path` below instead of
 * splitting part of it off as a "hostname". It also resolves to the right
 * scheme automatically in Expo Go (`exp://...`) vs. a standalone build
 * (`shelfcircle://...`).
 */
export function buildInviteUrl(token: string): string {
  return Linking.createURL(`invite/${token}`);
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
