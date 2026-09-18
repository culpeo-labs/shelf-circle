import { apiFetch } from './client';
import type {
  Book,
  BookSearchResult,
  BookStatus,
  BookWithEdition,
  CreateRecommendationInput,
  CreateUserInput,
  Friendship,
  LibraryEntry,
  LibraryShelf,
  FeedItem,
  Recommendation,
  ResolvedBookInput,
  SetBookStatusInput,
  User,
  UUID,
} from './types';

/** 404 means the token is valid but onboarding (`POST /users`) hasn't run yet. */
export const getMe = () => apiFetch<User>('/me');

export const createUser = (input: CreateUserInput) =>
  apiFetch<User>('/users', { method: 'POST', body: input });

export const getUser = (id: UUID) => apiFetch<User>(`/users/${id}`);

export const getUserByHandle = (handle: string) =>
  apiFetch<User>(`/users/by-handle/${encodeURIComponent(handle)}`);

/** Friends the other person by handle; the caller is implied by the auth token. */
export const createFriendship = (userHandle: string) =>
  apiFetch<Friendship>('/friendships', { method: 'POST', body: { user_handle: userHandle } });

export const searchBooks = (q: string, limit = 20) =>
  apiFetch<BookSearchResult[]>('/books/search', { query: { q, limit } });

export const resolveBookByReference = (source: string, sourceId: string) =>
  apiFetch<BookWithEdition>('/books/resolve', {
    method: 'POST',
    body: { source, source_id: sourceId },
  });

export const resolveManualBook = (input: ResolvedBookInput) =>
  apiFetch<BookWithEdition>('/books/resolve', { method: 'POST', body: input });

export const getBook = (id: UUID) => apiFetch<Book>(`/books/${id}`);

/** Upsert: a user has at most one status per book. */
export const setBookStatus = (input: SetBookStatusInput) =>
  apiFetch<BookStatus>('/book-statuses', { method: 'PUT', body: input });

export const listBookStatuses = (userId: UUID) =>
  apiFetch<BookStatus[]>(`/users/${userId}/book-statuses`);

export const getLibrary = (userId: UUID, shelf?: LibraryShelf) =>
  apiFetch<LibraryEntry[]>(`/users/${userId}/library`, { query: { shelf } });

export const createRecommendation = (input: CreateRecommendationInput) =>
  apiFetch<Recommendation>('/recommendations', { method: 'POST', body: input });

export const getRecommendationsInbox = (userId: UUID) =>
  apiFetch<Recommendation[]>(`/users/${userId}/recommendations/inbox`);

export const getFeed = (userId: UUID, opts: { limit?: number; before?: string } = {}) =>
  apiFetch<FeedItem[]>(`/users/${userId}/feed`, { query: opts });
