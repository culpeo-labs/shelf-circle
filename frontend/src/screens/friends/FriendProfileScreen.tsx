import { useNavigation } from '@react-navigation/native';
import type {
  NativeStackNavigationProp,
  NativeStackScreenProps,
} from '@react-navigation/native-stack';
import React, { useMemo } from 'react';
import { ActivityIndicator, Pressable, ScrollView, StyleSheet, Text, View } from 'react-native';

import type { LibraryEntry } from '../../api/types';
import { Avatar } from '../../components/Avatar';
import { BookCover } from '../../components/BookCover';
import { StarRating } from '../../components/StarRating';
import { useFriendLibrary, useFriendProfile } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'FriendProfile'>;

const SHELVES: { title: string; statuses: LibraryEntry['status'][] }[] = [
  { title: 'Reading', statuses: ['currently_reading'] },
  { title: 'Read', statuses: ['finished', 'did_not_finish'] },
  { title: 'Want to read', statuses: ['want_to_read'] },
];

export function FriendProfileScreen({ route }: Props) {
  const { friendshipId, displayName } = route.params;
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { data } = useFriendProfile(friendshipId);
  const name = data?.display_name ?? displayName;
  const shared = data?.share_shelves === true;
  const library = useFriendLibrary(friendshipId, shared);

  const shelves = useMemo(
    () =>
      SHELVES.map((s) => ({
        title: s.title,
        entries: (library.data ?? []).filter((e) => s.statuses.includes(e.status)),
      })).filter((s) => s.entries.length > 0),
    [library.data],
  );

  return (
    <ScrollView contentContainerStyle={styles.container}>
      <Avatar url={data?.avatar_url ?? null} name={name} size={72} />
      <Text style={styles.name}>{name}</Text>
      {data?.handle && <Text style={styles.handle}>@{data.handle}</Text>}

      {data && !shared && <Text style={styles.note}>{name} hasn't shared their bookshelves.</Text>}
      {shared && library.isLoading && <ActivityIndicator color="#3b6e5e" style={styles.spinner} />}
      {shared && library.isError && (
        <Text style={styles.note}>Couldn't load {name}'s bookshelves.</Text>
      )}
      {shared && library.data && shelves.length === 0 && (
        <Text style={styles.note}>{name}'s shelves are empty so far.</Text>
      )}

      {shelves.map((shelf) => (
        <View key={shelf.title} style={styles.shelf}>
          <Text style={styles.shelfTitle}>{shelf.title}</Text>
          {shelf.entries.map((entry) => (
            <Pressable
              key={entry.book.id}
              style={styles.row}
              onPress={() => navigation.navigate('BookDetail', { bookId: entry.book.id })}
            >
              <BookCover url={entry.book.cover_image_url} title={entry.book.title} />
              <View style={styles.rowText}>
                <Text style={styles.bookTitle} numberOfLines={2}>
                  {entry.book.title}
                </Text>
                {entry.book.author && <Text style={styles.author}>{entry.book.author}</Text>}
                {(entry.status === 'finished' || entry.status === 'did_not_finish') && (
                  <View style={styles.meta}>
                    <StarRating value={entry.rating} size={14} />
                    {entry.status === 'did_not_finish' && <Text style={styles.dnf}>DNF</Text>}
                  </View>
                )}
              </View>
            </Pressable>
          ))}
        </View>
      ))}
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: { alignItems: 'center', padding: 24, gap: 6, paddingTop: 48 },
  name: { fontSize: 20, fontWeight: '700', color: '#2b2a26', marginTop: 8 },
  handle: { fontSize: 14, color: '#6b6456' },
  note: { fontSize: 13, color: '#918a78', textAlign: 'center', marginTop: 24, lineHeight: 19 },
  spinner: { marginTop: 24 },
  shelf: { alignSelf: 'stretch', marginTop: 24, gap: 12 },
  shelfTitle: { fontSize: 13, fontWeight: '700', color: '#6b6456', textTransform: 'uppercase' },
  row: { flexDirection: 'row', gap: 12, alignItems: 'center' },
  rowText: { flex: 1, gap: 2 },
  bookTitle: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  author: { fontSize: 13, color: '#6b6456' },
  meta: { flexDirection: 'row', alignItems: 'center', gap: 8, marginTop: 2 },
  dnf: { fontSize: 11, fontWeight: '700', color: '#b3432b' },
});
