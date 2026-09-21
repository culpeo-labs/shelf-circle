import AsyncStorage from '@react-native-async-storage/async-storage';
import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';

import type { UUID } from '../api/types';

/**
 * There is no `GET /users/{id}/friends` endpoint yet (spec §9, §6.9 — a
 * known backend gap). Best-effort client-side derivation instead: remember
 * everyone we've explicitly friended, plus every distinct actor seen in the
 * feed. Good enough at friends-scale; will look stale if a friend added you
 * back without you seeing them in your feed yet.
 */
export interface FriendSummary {
  id: UUID;
  handle: string;
  display_name: string;
  avatar_url: string | null;
}

const STORAGE_KEY = 'sc_friends';

interface FriendsContextValue {
  friends: FriendSummary[];
  upsertFriend: (friend: FriendSummary) => void;
  upsertFriends: (friends: FriendSummary[]) => void;
}

const FriendsContext = createContext<FriendsContextValue | null>(null);

export function FriendsProvider({ children }: { children: React.ReactNode }) {
  const [byId, setById] = useState<Record<UUID, FriendSummary>>({});
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    void (async () => {
      const raw = await AsyncStorage.getItem(STORAGE_KEY);
      if (raw) setById(JSON.parse(raw));
      setLoaded(true);
    })();
  }, []);

  useEffect(() => {
    if (!loaded) return;
    void AsyncStorage.setItem(STORAGE_KEY, JSON.stringify(byId));
  }, [byId, loaded]);

  // Stable regardless of `byId` — both only ever use the functional setState
  // updater form, so callers (e.g. an effect's dependency array) can depend
  // on them without refiring every time a friend is added.
  const upsertFriend = useCallback((friend: FriendSummary) => {
    setById((prev) => ({ ...prev, [friend.id]: friend }));
  }, []);

  const upsertFriends = useCallback((friends: FriendSummary[]) => {
    setById((prev) => {
      const next = { ...prev };
      for (const f of friends) next[f.id] = f;
      return next;
    });
  }, []);

  const friends = useMemo(
    () => Object.values(byId).sort((a, b) => a.display_name.localeCompare(b.display_name)),
    [byId],
  );

  const value = useMemo<FriendsContextValue>(
    () => ({ friends, upsertFriend, upsertFriends }),
    [friends, upsertFriend, upsertFriends],
  );

  return <FriendsContext.Provider value={value}>{children}</FriendsContext.Provider>;
}

export function useFriends(): FriendsContextValue {
  const ctx = useContext(FriendsContext);
  if (!ctx) throw new Error('useFriends must be used within FriendsProvider');
  return ctx;
}
