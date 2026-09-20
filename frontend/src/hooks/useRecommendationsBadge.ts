import AsyncStorage from '@react-native-async-storage/async-storage';
import { useEffect, useState } from 'react';

import { useRecommendationsInbox } from './queries';

const SEEN_KEY = 'sc_recs_last_seen';

/**
 * The inbox has no "mark handled" concept (spec §10) — just a locally
 * tracked "newest I've opened the tab and seen" timestamp for the tab badge.
 */
export function useRecommendationsBadge() {
  const { data } = useRecommendationsInbox();
  const [lastSeen, setLastSeen] = useState<string | null>(null);

  useEffect(() => {
    void AsyncStorage.getItem(SEEN_KEY).then(setLastSeen);
  }, []);

  const unseenCount =
    data && lastSeen !== null
      ? data.filter((r) => r.created_at > lastSeen).length
      : data && lastSeen === null
        ? data.length
        : 0;

  async function markSeen() {
    if (!data || data.length === 0) return;
    const newest = data[0].created_at;
    await AsyncStorage.setItem(SEEN_KEY, newest);
    setLastSeen(newest);
  }

  return { unseenCount, markSeen };
}
