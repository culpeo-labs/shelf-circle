import { useNavigation } from '@react-navigation/native';
import React, { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';

import { ApiError } from '../../api/client';
import { getUserByHandle } from '../../api/endpoints';
import type { User } from '../../api/types';
import { Avatar } from '../../components/Avatar';
import { useFriends } from '../../friends/FriendsContext';
import { useCreateFriendship } from '../../hooks/queries';

export function AddFriendScreen() {
  const navigation = useNavigation();
  const { upsertFriend } = useFriends();
  const createFriendship = useCreateFriendship();

  const [handle, setHandle] = useState('');
  const [preview, setPreview] = useState<User | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    const trimmed = handle.trim().toLowerCase();
    setPreview(null);
    setPreviewError(null);
    if (trimmed.length === 0) return;
    const id = setTimeout(async () => {
      setChecking(true);
      try {
        const user = await getUserByHandle(trimmed);
        setPreview(user);
      } catch (e) {
        if (e instanceof ApiError && e.status === 404) setPreviewError('No such handle.');
      } finally {
        setChecking(false);
      }
    }, 400);
    return () => clearTimeout(id);
  }, [handle]);

  async function submit() {
    const trimmed = handle.trim().toLowerCase();
    if (!trimmed) return;
    try {
      await createFriendship.mutateAsync(trimmed);
      const friend = preview ?? (await getUserByHandle(trimmed));
      upsertFriend(friend);
      navigation.goBack();
    } catch {
      // surfaced via createFriendship.isError below
    }
  }

  return (
    <View style={styles.container}>
      <Text style={styles.label}>Friend's handle</Text>
      <TextInput
        style={styles.input}
        value={handle}
        onChangeText={setHandle}
        placeholder="ada"
        autoCapitalize="none"
        autoCorrect={false}
        autoFocus
      />

      {checking && <ActivityIndicator style={styles.previewSpinner} color="#3b6e5e" />}
      {preview && (
        <View style={styles.preview}>
          <Avatar url={preview.avatar_url} name={preview.display_name} />
          <Text style={styles.previewName}>{preview.display_name}</Text>
        </View>
      )}
      {previewError && <Text style={styles.error}>{previewError}</Text>}
      {createFriendship.isError && (
        <Text style={styles.error}>
          {createFriendship.error instanceof ApiError ? createFriendship.error.message : 'Could not add friend.'}
        </Text>
      )}

      <Pressable
        style={[styles.button, (!preview || createFriendship.isPending) && styles.buttonDisabled]}
        onPress={submit}
        disabled={!preview || createFriendship.isPending}
      >
        {createFriendship.isPending ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Add friend</Text>
        )}
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, padding: 20, gap: 14 },
  label: { fontSize: 13, fontWeight: '600', color: '#6b6456' },
  input: {
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 12,
    fontSize: 16,
    backgroundColor: '#fff',
  },
  previewSpinner: { alignSelf: 'flex-start' },
  preview: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  previewName: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  error: { color: '#b3432b' },
  button: { backgroundColor: '#3b6e5e', borderRadius: 8, paddingVertical: 14, alignItems: 'center', marginTop: 8 },
  buttonDisabled: { opacity: 0.5 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
});
