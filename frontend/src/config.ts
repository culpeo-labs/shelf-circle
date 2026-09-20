/**
 * Build-time config. Set these in `.env` (see `.env.example`) — Expo inlines
 * any `EXPO_PUBLIC_*` var at build time, so there's nothing to load at runtime.
 */

function required(name: string, value: string | undefined): string {
  if (!value) {
    throw new Error(
      `Missing ${name}. Copy .env.example to .env, fill it in, and restart the dev server.`,
    );
  }
  return value;
}

export const API_BASE_URL = required(
  'EXPO_PUBLIC_API_BASE_URL',
  process.env.EXPO_PUBLIC_API_BASE_URL,
).replace(/\/+$/, '');

/** Hanko Cloud project API URL, e.g. https://<project-id>.hanko.io — no trailing slash. */
export const HANKO_API_URL = required(
  'EXPO_PUBLIC_HANKO_API_URL',
  process.env.EXPO_PUBLIC_HANKO_API_URL,
).replace(/\/+$/, '');
