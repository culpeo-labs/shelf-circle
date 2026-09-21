import type { NativeStackScreenProps } from '@react-navigation/native-stack';
import React, { useState } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from 'react-native';

import { StarRating } from '../../components/StarRating';
import { useSetBookStatus } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'FinishBook'>;
type FinishStatus = 'finished' | 'did_not_finish';

export function FinishBookScreen({ route, navigation }: Props) {
  const { bookId, initialStatus, backdated } = route.params;
  const [status, setStatus] = useState<FinishStatus>(initialStatus ?? 'finished');
  const [rating, setRating] = useState<number | null>(null);
  const setBookStatus = useSetBookStatus();

  async function submit() {
    await setBookStatus.mutateAsync({ book_id: bookId, status, rating, backdated });
    navigation.popToTop();
    // popToTop lands on Main; MyBooks/BookDetail queries are already
    // invalidated by useSetBookStatus, so the Read shelf reflects this on next view.
  }

  return (
    <View style={styles.container}>
      {backdated && (
        <Text style={styles.backdatedNote}>This won't show up in your friends' timeline.</Text>
      )}

      <View style={styles.toggle}>
        <Pressable
          style={[styles.toggleOption, status === 'finished' && styles.toggleOptionActive]}
          onPress={() => setStatus('finished')}
        >
          <Text style={[styles.toggleText, status === 'finished' && styles.toggleTextActive]}>
            Finished
          </Text>
        </Pressable>
        <Pressable
          style={[styles.toggleOption, status === 'did_not_finish' && styles.toggleOptionActive]}
          onPress={() => setStatus('did_not_finish')}
        >
          <Text style={[styles.toggleText, status === 'did_not_finish' && styles.toggleTextActive]}>
            Didn't finish
          </Text>
        </Pressable>
      </View>

      <View style={styles.ratingBlock}>
        <Text style={styles.label}>Rating (optional)</Text>
        <StarRating value={rating} onChange={setRating} />
      </View>

      <Pressable
        style={[styles.button, setBookStatus.isPending && styles.buttonDisabled]}
        onPress={submit}
        disabled={setBookStatus.isPending}
      >
        {setBookStatus.isPending ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Save</Text>
        )}
      </Pressable>

      {setBookStatus.isError && <Text style={styles.error}>Could not save — try again.</Text>}
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, padding: 20, gap: 24 },
  backdatedNote: { fontSize: 13, color: '#6b6456', textAlign: 'center', marginBottom: -8 },
  toggle: { flexDirection: 'row', backgroundColor: '#efeae0', borderRadius: 8, padding: 3 },
  toggleOption: { flex: 1, paddingVertical: 10, borderRadius: 6, alignItems: 'center' },
  toggleOptionActive: { backgroundColor: '#fff' },
  toggleText: { color: '#6b6456', fontWeight: '500' },
  toggleTextActive: { color: '#2b2a26', fontWeight: '700' },
  ratingBlock: { alignItems: 'center', gap: 10 },
  label: { fontSize: 13, fontWeight: '600', color: '#6b6456' },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
  error: { color: '#b3432b', textAlign: 'center' },
});
