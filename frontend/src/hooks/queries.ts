import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import * as api from '../api/endpoints';
import type {
  CreateRecommendationInput,
  LibraryShelf,
  ResolvedBookInput,
  SetBookStatusInput,
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

export function useUser(userId: UUID | undefined) {
  return useQuery({
    queryKey: ['user', userId],
    queryFn: () => api.getUser(userId!),
    enabled: !!userId,
  });
}

export function useRecommendationsInbox() {
  const { user } = useAuth();
  return useQuery({
    queryKey: ['recommendations-inbox', user?.id],
    queryFn: () => api.getRecommendationsInbox(user!.id),
    enabled: !!user,
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
  return useMutation({
    mutationFn: () => api.createInvite(),
  });
}

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
    onSuccess: () => {
      if (user) void queryClient.invalidateQueries({ queryKey: ['feed', user.id] });
    },
  });
}
