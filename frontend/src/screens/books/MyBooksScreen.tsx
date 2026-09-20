import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import React, { useState } from 'react';
import { FlatList, Pressable, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../../api/client';
import type { LibraryEntry, LibraryShelf } from '../../api/types';
import { BookCover } from '../../components/BookCover';
import { StarRating } from '../../components/StarRating';
import { EmptyState, ErrorRetry, LoadingScreen } from '../../components/StatusViews';
import { useLibrary, useSetBookStatus } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Tab = Extract<LibraryShelf, 'reading' | 'read' | 'want_to_read'>;
const TABS: { key: Tab; label: string }[] = [
  { key: 'reading', label: 'Reading' },
  { key: 'read', label: 'Read' },
  { key: 'want_to_read', label: 'Want to read' },
];

export function MyBooksScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const [tab, setTab] = useState<Tab>('reading');
  const { data, isLoading, isError, error, refetch } = useLibrary(tab);
  const setStatus = useSetBookStatus();

  return (
    <View style={styles.flex}>
      <View style={styles.segmented}>
        {TABS.map((t) => (
          <Pressable
            key={t.key}
            style={[styles.segment, tab === t.key && styles.segmentActive]}
            onPress={() => setTab(t.key)}
          >
            <Text style={[styles.segmentText, tab === t.key && styles.segmentTextActive]}>{t.label}</Text>
          </Pressable>
        ))}
      </View>

      <View style={styles.header}>
        {tab === 'read' && (
          <HeaderButton label="+ Add a book I've read" onPress={() => navigation.navigate('AddPastRead')} />
        )}
        {(tab === 'want_to_read' || tab === 'reading') && (
          <HeaderButton label="+ Find a book" onPress={() => navigation.navigate('BookSearch')} />
        )}
      </View>

      {isLoading ? (
        <LoadingScreen />
      ) : isError ? (
        <ErrorRetry
          message={error instanceof ApiError ? error.message : 'Could not load your library.'}
          onRetry={() => void refetch()}
        />
      ) : !data || data.length === 0 ? (
        <EmptyState title={emptyCopy[tab]} />
      ) : (
        <FlatList
          data={data}
          keyExtractor={(item) => item.book.id}
          contentContainerStyle={styles.list}
          renderItem={({ item }) => (
            <LibraryRow
              entry={item}
              tab={tab}
              onPress={() => navigation.navigate('BookDetail', { bookId: item.book.id })}
              onStartReading={() =>
                setStatus.mutate({ book_id: item.book.id, status: 'currently_reading' })
              }
              onMarkFinished={() =>
                navigation.navigate('FinishBook', { bookId: item.book.id, initialStatus: 'finished' })
              }
            />
          )}
        />
      )}
    </View>
  );
}

const emptyCopy: Record<Tab, string> = {
  reading: 'Nothing in progress. Find a book to start reading.',
  read: "You haven't logged any finished books yet.",
  want_to_read: 'Nothing on your want-to-read list yet.',
};

function HeaderButton({ label, onPress }: { label: string; onPress: () => void }) {
  return (
    <Pressable onPress={onPress} style={styles.headerButton}>
      <Text style={styles.headerButtonText}>{label}</Text>
    </Pressable>
  );
}

function LibraryRow({
  entry,
  tab,
  onPress,
  onStartReading,
  onMarkFinished,
}: {
  entry: LibraryEntry;
  tab: Tab;
  onPress: () => void;
  onStartReading: () => void;
  onMarkFinished: () => void;
}) {
  return (
    <Pressable style={styles.row} onPress={onPress}>
      <BookCover url={entry.book.cover_image_url} title={entry.book.title} />
      <View style={styles.rowText}>
        <Text style={styles.title} numberOfLines={2}>
          {entry.book.title}
        </Text>
        {entry.book.author && <Text style={styles.author}>{entry.book.author}</Text>}

        {tab === 'reading' && (
          <View style={styles.progressTrack}>
            <View style={[styles.progressFill, { width: `${entry.progress_percent ?? 0}%` }]} />
          </View>
        )}
        {tab === 'read' && (
          <View style={styles.readMeta}>
            <StarRating value={entry.rating} size={14} />
            {entry.status === 'did_not_finish' && <Text style={styles.dnfBadge}>DNF</Text>}
          </View>
        )}
      </View>

      {tab === 'reading' && (
        <Pressable style={styles.rowAction} onPress={onMarkFinished}>
          <Text style={styles.rowActionText}>Finish</Text>
        </Pressable>
      )}
      {tab === 'want_to_read' && (
        <Pressable style={styles.rowAction} onPress={onStartReading}>
          <Text style={styles.rowActionText}>Start</Text>
        </Pressable>
      )}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  segmented: { flexDirection: 'row', margin: 16, backgroundColor: '#efeae0', borderRadius: 8, padding: 3 },
  segment: { flex: 1, paddingVertical: 8, borderRadius: 6, alignItems: 'center' },
  segmentActive: { backgroundColor: '#fff' },
  segmentText: { fontSize: 13, color: '#6b6456', fontWeight: '500' },
  segmentTextActive: { color: '#2b2a26', fontWeight: '600' },
  header: { paddingHorizontal: 16 },
  headerButton: { alignSelf: 'flex-start', paddingBottom: 12 },
  headerButtonText: { color: '#3b6e5e', fontWeight: '600' },
  list: { paddingHorizontal: 16, paddingBottom: 24, gap: 14 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  rowText: { flex: 1, gap: 4 },
  title: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  author: { fontSize: 13, color: '#6b6456' },
  progressTrack: { height: 5, borderRadius: 3, backgroundColor: '#efeae0', overflow: 'hidden', marginTop: 2 },
  progressFill: { height: '100%', backgroundColor: '#3b6e5e' },
  readMeta: { flexDirection: 'row', alignItems: 'center', gap: 8 },
  dnfBadge: {
    fontSize: 10,
    fontWeight: '700',
    color: '#b3432b',
    borderWidth: 1,
    borderColor: '#b3432b',
    borderRadius: 4,
    paddingHorizontal: 4,
    paddingVertical: 1,
  },
  rowAction: { paddingVertical: 6, paddingHorizontal: 10, borderWidth: 1, borderColor: '#3b6e5e', borderRadius: 6 },
  rowActionText: { color: '#3b6e5e', fontWeight: '600', fontSize: 12 },
});
