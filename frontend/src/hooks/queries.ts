import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import * as api from '../api/endpoints';
import type {
  CreateRecommendationInput,
  LibraryShelf,
  ResolvedBookInput,
  SetBookStatusInput,
  UpdateMeInput,
  UUID,
} from '../api/types';
import { useAuth } from '../auth/AuthContext';

const FEED_PAGE_SIZE = 50;

export function useFeed() {
  const { user } = useAuth();
  return useInfiniteQuery({
    queryKey: ['feed', user?.id],
    queryFn: ({ pageParam }: { pageParam?: string }) =>
      api.getFeed(user!.id, { limit: FEED_PAGE_SIZE, before: pageParam }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (lastPage) =>
      lastPage.length === FEED_PAGE_SIZE ? lastPage[lastPage.length - 1].created_at : undefined,
    enabled: !!user,
  });
}

export function useLibrary(shelf: LibraryShelf) {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['library', user?.id, shelf],
    queryFn: () => api.getLibrary(user!.id, shelf),
    enabled: !!user,
  });
}

/** A friend's library — the server 403s unless they've turned on sharing. */
export function useFriendLibrary(friendshipId: UUID | undefined, enabled: boolean) {
  return useQuery({
    queryKey: ['friend-library', friendshipId, 'all'],
    queryFn: () => api.getFriendLibrary(friendshipId!, 'all'),
    enabled: !!friendshipId && enabled,
  });
}

export function useUpdateMe() {
  const { updateUser } = useAuth();
  return useMutation({
    mutationFn: (input: UpdateMeInput) => api.updateMe(input),
    onSuccess: async (updated) => {
      await updateUser(updated);
    },
  });
}

export function useLibrarySystems() {
  return useQuery({ queryKey: ['library-systems'], queryFn: api.listLibrarySystems });
}

export function useMyLibrarySystem() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['my-library-system', user?.id],
    queryFn: api.getMyLibrarySystem,
    enabled: !!user,
  });
}

export function useSetMyLibrarySystem() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: (id: string | null) => api.setMyLibrarySystem(id),
    onSuccess: (mine) => {
      queryClient.setQueryData(['my-library-system', user?.id], mine);
      // Links are per-library.
      void queryClient.invalidateQueries({ queryKey: ['book-library-link'] });
    },
  });
}

/** The book's page in the user's library catalog. Only call once a library is chosen. */
export function useBookLibraryLink(bookId: UUID | undefined, libraryId: string | undefined) {
  return useQuery({
    queryKey: ['book-library-link', libraryId, bookId],
    queryFn: () => api.getBookLibraryLink(bookId!),
    enabled: !!bookId && !!libraryId,
    staleTime: 10 * 60 * 1000,
  });
}

/** The device's IANA time zone, so year boundaries match the user's calendar. */
export function deviceTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

export function useReadingStats(year: number) {
  const { user } = useAuth();
  const tz = deviceTimeZone();
  return useQuery({
    queryKey: ['reading-stats', user?.id, year, tz],
    queryFn: () => api.getReadingStats(year, tz),
    enabled: !!user,
  });
}

function useReadingGoalMutation<TInput>(fn: (input: TInput) => Promise<unknown>) {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['reading-stats', user?.id] }),
  });
}

export function useSetReadingGoal() {
  return useReadingGoalMutation(({ year, target }: { year: number; target: number }) =>
    api.setReadingGoal(year, target, deviceTimeZone()),
  );
}

export function useDeleteReadingGoal() {
  return useReadingGoalMutation((year: number) => api.deleteReadingGoal(year));
}

export function useBookStatuses() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['book-statuses', user?.id],
    queryFn: () => api.listBookStatuses(user!.id),
    enabled: !!user,
  });
}

export function useBook(bookId: UUID | undefined) {
  return useQuery({
    queryKey: ['book', bookId],
    queryFn: () => api.getBook(bookId!),
    enabled: !!bookId,
  });
}

/** A friend's profile, addressed by friendship. */
export function useFriendProfile(friendshipId: UUID | undefined) {
  return useQuery({
    queryKey: ['friend', friendshipId],
    queryFn: () => api.getFriend(friendshipId!),
    enabled: !!friendshipId,
  });
}

/**
 * The caller's friends, straight from the server (`GET /me/friends`) so a friend
 * who added *you* (e.g. scanned your invite QR) shows up too. Pass `pollMs`
 * on screens that wait for that to happen.
 */
export function useFriends(opts: { pollMs?: number } = {}) {
  const { user } = useAuth();
  const query = useQuery({
    queryKey: ['friends', user?.id],
    queryFn: api.listFriends,
    enabled: !!user,
    refetchInterval: opts.pollMs,
  });
  return { ...query, friends: query.data ?? [] };
}

export function useRecommendationsInbox() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['recommendations-inbox', user?.id],
    queryFn: () => api.getRecommendationsInbox(user!.id),
    enabled: !!user,
    // The tab badge lives on this query, so poll (only while the app is
    // foregrounded) rather than waiting for the user to open the tab.
    refetchInterval: 60_000,
  });
}

/** Invalidates everything a book-status change can affect. */
function useInvalidateAfterStatusChange() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return () => {
    if (!user) return;
    void queryClient.invalidateQueries({ queryKey: ['library', user.id] });
    void queryClient.invalidateQueries({ queryKey: ['book-statuses', user.id] });
    void queryClient.invalidateQueries({ queryKey: ['feed', user.id] });
    void queryClient.invalidateQueries({ queryKey: ['reading-stats', user.id] });
  };
}

export function useSetBookStatus() {
  const invalidate = useInvalidateAfterStatusChange();
  return useMutation({
    mutationFn: (input: SetBookStatusInput) => api.setBookStatus(input),
    onSuccess: invalidate,
  });
}

export function useResolveBookByReference() {
  return useMutation({
    mutationFn: ({ source, sourceId }: { source: string; sourceId: string }) =>
      api.resolveBookByReference(source, sourceId),
  });
}

export function useResolveManualBook() {
  return useMutation({
    mutationFn: (input: ResolvedBookInput) => api.resolveManualBook(input),
  });
}

export function useCreateRecommendation() {
  return useMutation({
    mutationFn: (input: CreateRecommendationInput) => api.createRecommendation(input),
  });
}

export function useCreateInvite() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: (reusable: boolean) => api.createInvite(reusable),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['my-invites', user?.id] }),
  });
}

/** Your invites that can still be used (so a reusable link can be found and revoked). */
export function useMyInvites() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['my-invites', user?.id],
    queryFn: api.listMyInvites,
    enabled: !!user,
  });
}

export function useRevokeInvite() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: (token: string) => api.revokeInvite(token),
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ['my-invites', user?.id] }),
  });
}

/**
 * People asking to join through your reusable invites. Nothing pushes to the
 * device, so poll (only while the app is open) — it also drives the Friends
 * tab badge.
 */
export function useFriendRequests() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['friend-requests', user?.id],
    queryFn: api.listFriendRequests,
    enabled: !!user,
    refetchInterval: 60_000,
  });
}

function useDecideRequest(fn: (id: string) => Promise<unknown>) {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: fn,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['friend-requests', user?.id] });
      void queryClient.invalidateQueries({ queryKey: ['friends', user?.id] });
      void queryClient.invalidateQueries({ queryKey: ['feed', user?.id] });
    },
  });
}

export const useApproveFriendRequest = () => useDecideRequest(api.approveFriendRequest);
export const useDeclineFriendRequest = () => useDecideRequest(api.declineFriendRequest);

/** Public preview of who an invite token is from — no auth needed. */
export function useInvitePreview(token: string | undefined) {
  return useQuery({
    queryKey: ['invite-preview', token],
    queryFn: () => api.getInvitePreview(token!),
    enabled: !!token,
  });
}

export function useAcceptInvite() {
  const queryClient = useQueryClient();
  const { user } = useAuth();
  return useMutation({
    mutationFn: (token: string) => api.acceptInvite(token),
    onSuccess: (result) => {
      if (!user) return;
      // A pending request changes nothing for you yet; only a real connection does.
      if (result.status !== 'friends') return;
      void queryClient.invalidateQueries({ queryKey: ['feed', user.id] });
      void queryClient.invalidateQueries({ queryKey: ['friends', user.id] });
    },
  });
}
