import { API_BASE_URL } from '../config';

/**
 * Every non-2xx backend response body is `{ error: string }` (see
 * `backend/src/error.rs`). `status` lets callers branch on specific cases
 * (404 = not found, 409 = conflict/duplicate, 502 = book provider down)
 * without parsing `message` strings.
 */
export class ApiError extends Error {
  status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

/**
 * Supplies the current session's bearer token. Set once by `AuthProvider` as
 * the session changes, read on every request — keeps the client a plain
 * module (no React context plumbing needed to call it from React Query hooks).
 */
let authToken: string | null = null;
export function setAuthToken(token: string | null) {
  authToken = token;
}

interface RequestOptions {
  method?: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE';
  body?: unknown;
  query?: Record<string, string | number | undefined>;
}

function buildUrl(path: string, query?: RequestOptions['query']): string {
  const url = new URL(API_BASE_URL + path);
  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value !== undefined) url.searchParams.set(key, String(value));
    }
  }
  return url.toString();
}

export async function apiFetch<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { Accept: 'application/json' };
  if (authToken) headers.Authorization = `Bearer ${authToken}`;
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';

  let response: Response;
  try {
    response = await fetch(buildUrl(path, options.query), {
      method: options.method ?? 'GET',
      headers,
      body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    });
  } catch {
    throw new ApiError(0, 'Could not reach the server. Check your connection and try again.');
  }

  if (response.status === 204) return undefined as T;

  const text = await response.text();
  const data = text ? JSON.parse(text) : undefined;

  if (!response.ok) {
    const message =
      typeof data?.error === 'string' && response.status !== 500
        ? data.error
        : 'Something went wrong. Please try again.';
    throw new ApiError(response.status, message);
  }

  return data as T;
}
