import { useNavigation } from '@react-navigation/native';
import type {
  NativeStackNavigationProp,
  NativeStackScreenProps,
} from '@react-navigation/native-stack';
import React, { useState } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../../api/client';
import { getUser } from '../../api/endpoints';
import { Avatar } from '../../components/Avatar';
import { useAuth } from '../../auth/AuthContext';
import { useFriends } from '../../friends/FriendsContext';
import { useAcceptInvite, useInvitePreview } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'AcceptInvite'>;

/** Reached either by scanning a QR code (ScanInviteScreen) or by opening a
 * shelfcircle://invite/{token} link directly (see App.tsx's linking config). */
export function AcceptInviteScreen({ route }: Props) {
  const { token } = route.params;
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { user: me } = useAuth();
  const { upsertFriend } = useFriends();
  const preview = useInvitePreview(token);
  const acceptInvite = useAcceptInvite();
  const [acceptError, setAcceptError] = useState<string | null>(null);

  async function accept() {
    setAcceptError(null);
    try {
      const friendship = await acceptInvite.mutateAsync(token);
      // The accept itself has already succeeded server-side at this point
      // (friendship created, token consumed) — a failure hydrating the
      // friend's profile below shouldn't be reported as "could not accept
      // this invite" (which would be actively misleading: retrying would
      // just fail on the now-used token) or block leaving this screen.
      const friendId =
        friendship.user_a_id === me?.id ? friendship.user_b_id : friendship.user_a_id;
      try {
        const friend = await getUser(friendId);
        upsertFriend(friend);
      } catch {
        // FriendsContext will still pick them up later via the feed's
        // best-effort actor discovery.
      }
      navigation.popToTop();
    } catch (e) {
      setAcceptError(e instanceof ApiError ? e.message : 'Could not accept this invite.');
    }
  }

  if (preview.isLoading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator size="large" color="#3b6e5e" />
      </View>
    );
  }

  if (preview.isError || !preview.data) {
    return (
      <View style={styles.centered}>
        <Text style={styles.message}>
          {preview.error instanceof ApiError && preview.error.status === 404
            ? 'This invite is invalid, expired, or has already been used.'
            : 'Could not load this invite.'}
        </Text>
        <Pressable style={styles.secondaryButton} onPress={() => navigation.popToTop()}>
          <Text style={styles.secondaryButtonText}>Done</Text>
        </Pressable>
      </View>
    );
  }

  return (
    <View style={styles.centered}>
      <Avatar url={preview.data.avatar_url} name={preview.data.display_name} size={72} />
      <Text style={styles.title}>{preview.data.display_name} wants to be your friend</Text>
      <Text style={styles.subtitle}>on Shelf Circle</Text>

      {acceptError && <Text style={styles.error}>{acceptError}</Text>}

      <Pressable
        style={[styles.button, acceptInvite.isPending && styles.buttonDisabled]}
        onPress={accept}
        disabled={acceptInvite.isPending}
      >
        {acceptInvite.isPending ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Accept</Text>
        )}
      </Pressable>
      <Pressable onPress={() => navigation.popToTop()} disabled={acceptInvite.isPending}>
        <Text style={styles.link}>Not now</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  centered: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, padding: 24 },
  title: { fontSize: 18, fontWeight: '700', color: '#2b2a26', textAlign: 'center' },
  subtitle: { fontSize: 14, color: '#6b6456', marginTop: -8 },
  message: { fontSize: 15, color: '#6b6456', textAlign: 'center' },
  error: { color: '#b3432b', textAlign: 'center' },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    paddingHorizontal: 40,
    alignItems: 'center',
    marginTop: 8,
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
  secondaryButton: {
    borderWidth: 1,
    borderColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 10,
    paddingHorizontal: 20,
    marginTop: 8,
  },
  secondaryButtonText: { color: '#3b6e5e', fontWeight: '600' },
  link: { color: '#3b6e5e', fontWeight: '500' },
});
