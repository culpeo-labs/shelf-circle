import { apiFetch } from './client';
import type {
  AcceptResult,
  AvatarUploadTicket,
  Book,
  BookLibraryLink,
  BookSearchResult,
  BookStatus,
  BookWithEdition,
  CreateRecommendationInput,
  CreateUserInput,
  Friend,
  FriendProfile,
  FriendRequest,
  Invite,
  InvitePreview,
  MyInvite,
  LibraryEntry,
  LibrarySystem,
  LibraryShelf,
  MyLibrarySystem,
  FeedItem,
  ReadingGoal,
  ReadingStats,
  RecommendationItem,
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

/** Permanently deletes the caller's account and all their data (and their sign-in). */
export const deleteAccount = () => apiFetch<void>('/me', { method: 'DELETE' });

export const createUser = (input: CreateUserInput) =>
  apiFetch<User>('/users', { method: 'POST', body: input });

/** Everyone the caller is friends with, whichever side created the invite. */
export const listFriends = () => apiFetch<Friend[]>('/me/friends');

export const getFriend = (friendshipId: UUID) =>
  apiFetch<FriendProfile>(`/friends/${friendshipId}`);

/** A friend's shelves — 403 unless they've turned on sharing. */
export const getFriendLibrary = (friendshipId: UUID, shelf?: LibraryShelf) =>
  apiFetch<LibraryEntry[]>(`/friends/${friendshipId}/library`, { query: { shelf } });

/** Creates a new invite (share as a QR code or link). Single-use by default;
 * `reusable` makes an "anyone with the link" invite whose joiners you approve. */
export const createInvite = (reusable = false) =>
  apiFetch<Invite>('/invites', { method: 'POST', query: reusable ? { reusable: 'true' } : {} });

export const listMyInvites = () => apiFetch<MyInvite[]>('/invites');

/** Stops an invite from being used (yours only). */
export const revokeInvite = (token: string) =>
  apiFetch<void>(`/invites/${encodeURIComponent(token)}`, { method: 'DELETE' });

/** Public — no auth token needed. 404 if the token is invalid/expired/used/revoked. */
export const getInvitePreview = (token: string) =>
  apiFetch<InvitePreview>(`/invites/${encodeURIComponent(token)}`);

/** Accepts an invite: `friends` right away for a single-use invite, `pending`
 * for a reusable one until the inviter approves. */
export const acceptInvite = (token: string) =>
  apiFetch<AcceptResult>(`/invites/${encodeURIComponent(token)}/accept`, { method: 'POST' });

export const listFriendRequests = () => apiFetch<FriendRequest[]>('/me/friend-requests');

export const approveFriendRequest = (id: UUID) =>
  apiFetch<AcceptResult>(`/friend-requests/${id}/approve`, { method: 'POST' });

export const declineFriendRequest = (id: UUID) =>
  apiFetch<void>(`/friend-requests/${id}/decline`, { method: 'POST' });

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
  apiFetch<unknown>('/recommendations', { method: 'POST', body: input });

export const getRecommendationsInbox = (userId: UUID) =>
  apiFetch<RecommendationItem[]>(`/users/${userId}/recommendations/inbox`);

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
