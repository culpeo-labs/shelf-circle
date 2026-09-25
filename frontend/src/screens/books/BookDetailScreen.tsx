import type {
  NativeStackNavigationProp,
  NativeStackScreenProps,
} from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import Slider from '@react-native-community/slider';
import React, { useMemo, useState } from 'react';
import { Linking, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import type { BookLibraryLink, ReadingStatus } from '../../api/types';
import { BookCover } from '../../components/BookCover';
import { StarRating } from '../../components/StarRating';
import { ErrorRetry, LoadingScreen } from '../../components/StatusViews';
import {
  useBook,
  useBookLibraryLink,
  useBookStatuses,
  useMyLibrarySystem,
  useSetBookStatus,
} from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'BookDetail'>;

/** Descriptions longer than this get a Read more toggle. */
const COLLAPSED_CHARS = 300;

const SHELVES: { key: ReadingStatus; label: string }[] = [
  { key: 'want_to_read', label: 'Want to read' },
  { key: 'currently_reading', label: 'Reading' },
  { key: 'finished', label: 'Finished' },
  { key: 'did_not_finish', label: "Didn't finish" },
];

export function BookDetailScreen({ route }: Props) {
  const { bookId } = route.params;
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();

  const book = useBook(bookId);
  const statuses = useBookStatuses();
  const setStatus = useSetBookStatus();
  const myLibrary = useMyLibrarySystem();
  const library = myLibrary.data?.library_system ?? null;
  const libraryLink = useBookLibraryLink(bookId, library?.id);

  const myStatus = useMemo(
    () => statuses.data?.find((s) => s.book_id === bookId) ?? null,
    [statuses.data, bookId],
  );
  const [localProgress, setLocalProgress] = useState<number | null>(null);
  const progress = localProgress ?? myStatus?.progress_percent ?? 0;

  if (book.isLoading) return <LoadingScreen />;
  if (book.isError || !book.data) {
    return <ErrorRetry message="Couldn't load this book." onRetry={() => void book.refetch()} />;
  }

  function selectShelf(status: ReadingStatus) {
    if (status === 'finished' || status === 'did_not_finish') {
      navigation.navigate('FinishBook', { bookId, initialStatus: status });
      return;
    }
    setStatus.mutate({ book_id: bookId, status });
  }

  return (
    <ScrollView contentContainerStyle={styles.container}>
      <View style={styles.header}>
        <BookCover
          url={book.data.cover_image_url}
          title={book.data.canonical_title}
          width={100}
          height={150}
        />
        <View style={styles.headerText}>
          <Text style={styles.title}>{book.data.canonical_title}</Text>
          {book.data.primary_author && (
            <Text style={styles.author}>{book.data.primary_author}</Text>
          )}
        </View>
      </View>

      {book.data.description && <Description text={book.data.description} />}

      <View style={styles.section}>
        <Text style={styles.sectionLabel}>Your shelf</Text>
        <View style={styles.shelfRow}>
          {SHELVES.map((s) => (
            <Pressable
              key={s.key}
              style={[styles.shelfOption, myStatus?.status === s.key && styles.shelfOptionActive]}
              onPress={() => selectShelf(s.key)}
            >
              <Text
                style={[styles.shelfText, myStatus?.status === s.key && styles.shelfTextActive]}
              >
                {s.label}
              </Text>
            </Pressable>
          ))}
        </View>

        {myStatus?.backdated && (
          <Text style={styles.backdatedBadge}>
            Logged from backlog · not shown in your friends' timeline
          </Text>
        )}

        {myStatus?.status === 'currently_reading' && (
          <View style={styles.progressBlock}>
            <Text style={styles.progressLabel}>{Math.round(progress)}% done</Text>
            <Slider
              minimumValue={0}
              maximumValue={100}
              step={5}
              value={progress}
              onValueChange={setLocalProgress}
              onSlidingComplete={(value) =>
                setStatus.mutate({
                  book_id: bookId,
                  status: 'currently_reading',
                  progress_percent: value,
                })
              }
              minimumTrackTintColor="#3b6e5e"
              maximumTrackTintColor="#e5e1d8"
            />
          </View>
        )}

        {(myStatus?.status === 'finished' || myStatus?.status === 'did_not_finish') && (
          <View style={styles.progressBlock}>
            <StarRating value={myStatus.rating} />
          </View>
        )}
      </View>

      <Pressable
        style={styles.recommendButton}
        onPress={() => navigation.navigate('RecommendToFriend', { bookId })}
      >
        <Text style={styles.recommendButtonText}>Recommend to a friend</Text>
      </Pressable>

      {myLibrary.isSuccess && (
        <LibraryButton
          libraryName={library?.name ?? null}
          loading={!!library && libraryLink.isLoading}
          link={libraryLink.data ?? null}
          onChoose={() => navigation.navigate('ChooseLibrary')}
        />
      )}

      <View style={styles.section}>
        <Text style={styles.sectionLabel}>Coming soon</Text>
        <View style={styles.disabledRow}>
          <Text style={styles.disabledText}>Buy a copy</Text>
        </View>
      </View>
    </ScrollView>
  );
}

/** The book's blurb, clamped with a Read more toggle when it's long. Rendered
 * only when a source had a description. */
function Description({ text }: { text: string }) {
  const [expanded, setExpanded] = useState(false);
  const long = text.length > COLLAPSED_CHARS;
  return (
    <View style={styles.section}>
      <Text style={styles.sectionLabel}>About this book</Text>
      <Text style={styles.description} numberOfLines={long && !expanded ? 6 : undefined}>
        {text}
      </Text>
      {long && (
        <Pressable onPress={() => setExpanded((e) => !e)} hitSlop={8}>
          <Text style={styles.readMore}>{expanded ? 'Show less' : 'Read more'}</Text>
        </Pressable>
      )}
    </View>
  );
}

/** "Get it at your library": the book's record when the catalog has it, a
 * catalog search otherwise, or a prompt to pick a library first. */
function LibraryButton({
  libraryName,
  loading,
  link,
  onChoose,
}: {
  libraryName: string | null;
  loading: boolean;
  link: BookLibraryLink | null;
  onChoose: () => void;
}) {
  if (!libraryName) {
    return (
      <Pressable style={styles.libraryButton} onPress={onChoose}>
        <Text style={styles.libraryButtonText}>Choose your library to find this book</Text>
      </Pressable>
    );
  }
  if (loading) {
    return (
      <View style={styles.libraryButton}>
        <Text style={styles.libraryHint}>Checking {libraryName}…</Text>
      </View>
    );
  }
  // Lookup failed at the HTTP level (link null): nothing to open, but don't hide the row.
  if (!link) {
    return (
      <Pressable style={styles.libraryButton} onPress={onChoose}>
        <Text style={styles.libraryHint}>Couldn't check {libraryName} · change library</Text>
      </Pressable>
    );
  }
  const label = link.found
    ? `Get it at ${libraryName}`
    : link.lookup_failed
      ? `Search ${libraryName}'s catalog`
      : `Not found at ${libraryName} · search anyway`;
  return (
    <Pressable style={styles.libraryButton} onPress={() => void Linking.openURL(link.url)}>
      <Text style={styles.libraryButtonText}>{label}</Text>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  container: { padding: 20, gap: 24 },
  header: { flexDirection: 'row', gap: 16 },
  headerText: { flex: 1, justifyContent: 'center', gap: 4 },
  title: { fontSize: 20, fontWeight: '700', color: '#2b2a26' },
  author: { fontSize: 15, color: '#6b6456' },
  section: { gap: 10 },
  description: { fontSize: 14, lineHeight: 21, color: '#2b2a26' },
  readMore: { color: '#3b6e5e', fontWeight: '600', fontSize: 14 },
  sectionLabel: { fontSize: 13, fontWeight: '600', color: '#6b6456', textTransform: 'uppercase' },
  shelfRow: { flexDirection: 'row', flexWrap: 'wrap', gap: 8 },
  shelfOption: {
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 20,
    paddingVertical: 8,
    paddingHorizontal: 14,
  },
  shelfOptionActive: { backgroundColor: '#3b6e5e', borderColor: '#3b6e5e' },
  shelfText: { color: '#2b2a26', fontSize: 13, fontWeight: '500' },
  shelfTextActive: { color: '#fff' },
  backdatedBadge: { fontSize: 12, color: '#6b6456', marginTop: 8 },
  progressBlock: { marginTop: 8, gap: 6 },
  progressLabel: { fontSize: 13, color: '#6b6456' },
  recommendButton: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
  },
  recommendButtonText: { color: '#fff', fontWeight: '600', fontSize: 15 },
  libraryButton: {
    borderWidth: 1,
    borderColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
  },
  libraryButtonText: { color: '#3b6e5e', fontWeight: '600', fontSize: 15 },
  libraryHint: { color: '#918a78', fontSize: 14 },
  disabledRow: {
    borderWidth: 1,
    borderColor: '#e5e1d8',
    borderRadius: 8,
    paddingVertical: 12,
    paddingHorizontal: 14,
  },
  disabledText: { color: '#b3ab99' },
});
