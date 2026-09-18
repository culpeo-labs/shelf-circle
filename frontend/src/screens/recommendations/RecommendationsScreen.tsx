import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useFocusEffect, useNavigation } from '@react-navigation/native';
import React, { useCallback } from 'react';
import { FlatList, Pressable, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../../api/client';
import type { Recommendation } from '../../api/types';
import { Avatar } from '../../components/Avatar';
import { BookCover } from '../../components/BookCover';
import { EmptyState, ErrorRetry, LoadingScreen } from '../../components/StatusViews';
import { useBook, useRecommendationsInbox, useUser } from '../../hooks/queries';
import { useRecommendationsBadge } from '../../hooks/useRecommendationsBadge';
import type { RootStackParamList } from '../../navigation/types';
import { relativeTime } from '../../utils/time';

export function RecommendationsScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { data, isLoading, isError, error, refetch } = useRecommendationsInbox();
  const { markSeen } = useRecommendationsBadge();

  useFocusEffect(
    useCallback(() => {
      void markSeen();
    }, [markSeen])
  );

  if (isLoading) return <LoadingScreen />;
  if (isError) {
    return (
      <ErrorRetry
        message={error instanceof ApiError ? error.message : 'Could not load recommendations.'}
        onRetry={() => void refetch()}
      />
    );
  }
  if (!data || data.length === 0) {
    return <EmptyState title="No recommendations yet" />;
  }

  return (
    <FlatList
      data={data}
      keyExtractor={(item) => item.id}
      contentContainerStyle={styles.list}
      renderItem={({ item }) => (
        <RecommendationRow item={item} onPress={() => navigation.navigate('BookDetail', { bookId: item.book_id })} />
      )}
    />
  );
}

function RecommendationRow({ item, onPress }: { item: Recommendation; onPress: () => void }) {
  const sender = useUser(item.from_user_id);
  const book = useBook(item.book_id);

  return (
    <Pressable style={styles.row} onPress={onPress}>
      <Avatar url={sender.data?.avatar_url ?? null} name={sender.data?.display_name ?? '?'} size={36} />
      <BookCover url={book.data?.cover_image_url ?? null} title={book.data?.canonical_title ?? ''} />
      <View style={styles.rowText}>
        <Text style={styles.sender}>{sender.data?.display_name ?? 'Someone'}</Text>
        <Text style={styles.title} numberOfLines={2}>
          {book.data?.canonical_title ?? 'A book'}
        </Text>
        {item.note && (
          <Text style={styles.note} numberOfLines={2}>
            "{item.note}"
          </Text>
        )}
        <Text style={styles.time}>{relativeTime(item.created_at)}</Text>
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  list: { padding: 16, gap: 16 },
  row: { flexDirection: 'row', alignItems: 'flex-start', gap: 10 },
  rowText: { flex: 1, gap: 3 },
  sender: { fontSize: 12, color: '#6b6456', fontWeight: '600' },
  title: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  note: { fontSize: 13, color: '#6b6456', fontStyle: 'italic' },
  time: { fontSize: 11, color: '#918a78' },
});
