import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import React, { useState } from 'react';
import { ActivityIndicator, StyleSheet, View } from 'react-native';

import { resolveBookByReference } from '../../api/endpoints';
import { BookSearchResults } from '../../components/BookSearchResults';
import type { RootStackParamList } from '../../navigation/types';

export function BookSearchScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const [resolving, setResolving] = useState(false);

  async function onSelect(result: { source: string; source_id: string }) {
    if (resolving) return;
    setResolving(true);
    try {
      const resolved = await resolveBookByReference(result.source, result.source_id);
      navigation.replace('BookDetail', { bookId: resolved.id });
    } finally {
      setResolving(false);
    }
  }

  return (
    <View style={styles.flex}>
      <BookSearchResults onSelect={onSelect} />
      {resolving && (
        <View style={styles.overlay}>
          <ActivityIndicator size="large" color="#3b6e5e" />
        </View>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  overlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    backgroundColor: 'rgba(250,248,243,0.7)',
    alignItems: 'center',
    justifyContent: 'center',
  },
});
