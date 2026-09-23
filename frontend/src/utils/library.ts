/**
 * "Get it at your library": a link to the book in the library's online
 * catalog. No API calls and no availability lookup — the catalog does that
 * once the user lands there. Hardcoded to Seattle Public Library for now
 * (every BiblioCommons library has the same `https://<slug>.bibliocommons.com`
 * catalog and search URL shape, so supporting other libraries later means
 * making this a setting, not changing the URL builder).
 */
export const LIBRARY = { name: 'Seattle Public Library', slug: 'seattle' } as const;

/** Catalog search for the book; title + author is more forgiving across
 * editions than an ISBN, which only matches the exact edition we happen to have. */
export function libraryCatalogUrl(title: string, author: string | null): string {
  const query = [title, author].filter(Boolean).join(' ');
  return `https://${LIBRARY.slug}.bibliocommons.com/v2/search?query=${encodeURIComponent(query)}&searchType=smart`;
}
