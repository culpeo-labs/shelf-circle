import { apiFetch } from './client';
import type {
  AvatarUploadTicket,
  Book,
  BookLibraryLink,
  BookSearchResult,
  BookStatus,
  BookWithEdition,
  CreateRecommendationInput,
  CreateUserInput,
  Friendship,
  Invite,
  InvitePreview,
  LibraryEntry,
  LibrarySystem,
  LibraryShelf,
  MyLibrarySystem,
  FeedItem,
  ReadingGoal,
  ReadingStats,
  Recommendation,
  ResolvedBookInput,
  SetBookStatusInput,
  UpdateMeInput,
  User,
  UUID,
} from './types';

/** 404 means the token is valid but onboarding (`POST /users`) hasn't run yet. */
export const getMe = () => apiFetch<User>('/me');

export const updateMe = (input: UpdateMeInput) =>
  apiFetch<User>('/me', { method: 'PATCH', body: input });

export const createAvatarUpload = () =>
  apiFetch<AvatarUploadTicket>('/me/avatar-upload', { method: 'POST' });

export const createUser = (input: CreateUserInput) =>
  apiFetch<User>('/users', { method: 'POST', body: input });

export const getUser = (id: UUID) => apiFetch<User>(`/users/${id}`);

/** Everyone the caller is friends with, whichever side created the invite. */
export const listFriends = () => apiFetch<User[]>('/me/friends');

/** Creates a new invite token for the caller (share as a QR code or link). */
export const createInvite = () => apiFetch<Invite>('/invites', { method: 'POST' });

/** Public — no auth token needed. 404 if the token is invalid/expired/used. */
export const getInvitePreview = (token: string) =>
  apiFetch<InvitePreview>(`/invites/${encodeURIComponent(token)}`);

/** Accepts an invite: creates the friendship, marks the token used. */
export const acceptInvite = (token: string) =>
  apiFetch<Friendship>(`/invites/${encodeURIComponent(token)}/accept`, { method: 'POST' });

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

export const listLibrarySystems = () => apiFetch<LibrarySystem[]>('/library-systems');

export const getMyLibrarySystem = () => apiFetch<MyLibrarySystem>('/me/library-system');

/** `null` clears the choice. */
export const setMyLibrarySystem = (id: string | null) =>
  apiFetch<MyLibrarySystem>('/me/library-system', {
    method: 'PUT',
    body: { library_system: id },
  });

/** Needs a library chosen first (400 otherwise). */
export const getBookLibraryLink = (bookId: UUID) =>
  apiFetch<BookLibraryLink>(`/books/${bookId}/library-link`);

export const getReadingStats = (year: number, tz: string) =>
  apiFetch<ReadingStats>('/me/reading-stats', { query: { year, tz } });

export const setReadingGoal = (year: number, targetCount: number, timeZone: string) =>
  apiFetch<ReadingGoal>(`/me/reading-goals/${year}`, {
    method: 'PUT',
    body: { target_count: targetCount, time_zone: timeZone },
  });

export const deleteReadingGoal = (year: number) =>
  apiFetch<void>(`/me/reading-goals/${year}`, { method: 'DELETE' });
