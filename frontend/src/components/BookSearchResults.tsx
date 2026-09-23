import { useQuery } from '@tanstack/react-query';
import React, { useEffect, useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';

import { ApiError } from '../api/client';
import { searchBooks } from '../api/endpoints';
import type { BookSearchResult } from '../api/types';
import { BookCover } from './BookCover';
import { languageName } from '../utils/language';

/** Debounced free-text search against `/books/search`, rendered as a tappable list. */
export function BookSearchResults({ onSelect }: { onSelect: (result: BookSearchResult) => void }) {
  const [query, setQuery] = useState('');
  const [debounced, setDebounced] = useState('');

  useEffect(() => {
    const id = setTimeout(() => setDebounced(query.trim()), 350);
    return () => clearTimeout(id);
  }, [query]);

  const { data, isFetching, isError, error } = useQuery({
    queryKey: ['book-search', debounced],
    queryFn: () => searchBooks(debounced),
    enabled: debounced.length > 0,
  });

  return (
    <View style={styles.flex}>
      <TextInput
        placeholderTextColor="#918a78"
        style={styles.input}
        placeholder="Search by title, author, or ISBN"
        value={query}
        onChangeText={setQuery}
        autoCapitalize="none"
        autoCorrect={false}
        autoFocus
      />
      <Text style={styles.hint}>Results come from Open Library and Google Books.</Text>

      {debounced.length === 0 ? (
        <View style={styles.centered}>
          <Text style={styles.dim}>Start typing to search.</Text>
        </View>
      ) : isFetching ? (
        <View style={styles.centered}>
          <ActivityIndicator color="#3b6e5e" />
        </View>
      ) : isError ? (
        <View style={styles.centered}>
          <Text style={styles.dim}>
            {error instanceof ApiError && error.status === 502
              ? 'Search is having trouble, try again.'
              : 'Something went wrong.'}
          </Text>
        </View>
      ) : !data || data.length === 0 ? (
        <View style={styles.centered}>
          <Text style={styles.dim}>No results.</Text>
        </View>
      ) : (
        <FlatList
          data={data}
          keyExtractor={(item) => `${item.source}:${item.source_id}`}
          renderItem={({ item }) => <ResultRow result={item} onPress={() => onSelect(item)} />}
          contentContainerStyle={styles.list}
          keyboardShouldPersistTaps="handled"
        />
      )}
    </View>
  );
}

function ResultRow({ result, onPress }: { result: BookSearchResult; onPress: () => void }) {
  return (
    <Pressable style={styles.row} onPress={onPress}>
      <BookCover url={result.cover_image_url} title={result.title} />
      <View style={styles.rowText}>
        <Text style={styles.title} numberOfLines={2}>
          {result.title}
        </Text>
        <Text style={styles.meta} numberOfLines={1}>
          {[result.authors.join(', '), result.first_publish_year].filter(Boolean).join(' · ')}
        </Text>
        {result.languages.length > 0 && (
          <Text style={styles.languages} numberOfLines={1}>
            {result.languages.map(languageName).join(' · ')}
          </Text>
        )}
      </View>
    </Pressable>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  input: {
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 12,
    fontSize: 16,
    backgroundColor: '#fff',
    color: '#2b2a26',
    marginHorizontal: 16,
    marginTop: 16,
  },
  hint: { fontSize: 12, color: '#918a78', marginHorizontal: 16, marginTop: 6, marginBottom: 4 },
  centered: { flex: 1, alignItems: 'center', justifyContent: 'center' },
  dim: { color: '#6b6456' },
  list: { padding: 16, gap: 14 },
  row: { flexDirection: 'row', gap: 12 },
  rowText: { flex: 1, gap: 3, justifyContent: 'center' },
  title: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  meta: { fontSize: 13, color: '#6b6456' },
  languages: { fontSize: 11, color: '#918a78' },
});
