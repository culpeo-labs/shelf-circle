import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import React, { useEffect, useMemo } from 'react';
import { FlatList, Pressable, RefreshControl, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../../api/client';
import type { FeedItem } from '../../api/types';
import { useAuth } from '../../auth/AuthContext';
import { Avatar } from '../../components/Avatar';
import { BookCover } from '../../components/BookCover';
import { EmptyState, ErrorRetry, LoadingScreen } from '../../components/StatusViews';
import { useFriends } from '../../friends/FriendsContext';
import { useFeed } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';
import { relativeTime } from '../../utils/time';

export function TimelineScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { user } = useAuth();
  const { upsertFriends } = useFriends();
  const { data, isLoading, isError, error, refetch, isRefetching, fetchNextPage, hasNextPage, isFetchingNextPage } =
    useFeed();

  const items = useMemo(() => data?.pages.flat() ?? [], [data]);
  // Duplicate created_at timestamps across pages are possible; de-dupe by id.
  const deduped = useMemo(() => {
    const seen = new Set<string>();
    return items.filter((item) => (seen.has(item.id) ? false : (seen.add(item.id), true)));
  }, [items]);

  useEffect(() => {
    // There's no "list my friends" endpoint (see FriendsContext) — feed
    // actors are one of the two ways we discover who's actually a friend.
    const others = deduped.filter((item) => item.actor.id !== user?.id).map((item) => item.actor);
    if (others.length > 0) upsertFriends(others);
  }, [deduped, user?.id]);

  if (isLoading) return <LoadingScreen />;
  if (isError) {
    return (
      <ErrorRetry
        message={error instanceof ApiError ? error.message : 'Could not load your timeline.'}
        onRetry={() => void refetch()}
      />
    );
  }

  if (deduped.length === 0) {
    return (
      <EmptyState
        title="Your timeline is quiet. Add friends or shelve a book."
        action={{ label: 'Find a book', onPress: () => navigation.navigate('BookSearch') }}
      />
    );
  }

  return (
    <FlatList
      data={deduped}
      keyExtractor={(item) => item.id}
      renderItem={({ item }) => (
        <FeedRow item={item} onPress={() => navigation.navigate('BookDetail', { bookId: item.book.id })} />
      )}
      refreshControl={<RefreshControl refreshing={isRefetching} onRefresh={() => void refetch()} />}
      onEndReached={() => {
        if (hasNextPage && !isFetchingNextPage) void fetchNextPage();
      }}
      onEndReachedThreshold={0.4}
      contentContainerStyle={styles.list}
    />
  );
}

function FeedRow({ item, onPress }: { item: FeedItem; onPress: () => void }) {
  return (
    <Pressable style={styles.row} onPress={onPress}>
      <Avatar url={item.actor.avatar_url} name={item.actor.display_name} size={36} />
      <View style={styles.rowText}>
        <Text style={styles.rowLine}>
          <Text style={styles.actorName}>{item.actor.display_name}</Text> {item.verb}{' '}
          <Text style={styles.bookTitle}>{item.book.title}</Text>
        </Text>
        <Text style={styles.time}>{relativeTime(item.created_at)}</Text>
      </View>
      <BookCover url={item.book.cover_image_url} title={item.book.title} width={36} height={54} />
    </Pressable>
  );
}

const styles = StyleSheet.create({
  list: { padding: 16, gap: 12 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12, paddingVertical: 6 },
  rowText: { flex: 1, gap: 2 },
  rowLine: { fontSize: 15, color: '#2b2a26' },
  actorName: { fontWeight: '600' },
  bookTitle: { fontStyle: 'italic' },
  time: { fontSize: 12, color: '#918a78' },
});
