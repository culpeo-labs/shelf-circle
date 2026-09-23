import { useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import React, { useEffect, useState } from 'react';
import { ActivityIndicator, Pressable, Share, StyleSheet, Text, View } from 'react-native';
import QRCode from 'react-native-qrcode-svg';

import { ApiError } from '../../api/client';
import { useCreateInvite, useFriends } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';
import { buildInviteUrl } from '../../utils/inviteLink';

/**
 * "Add a friend" is invite-based, not handle/search-based (see friends.md):
 * generate a token, show it as a QR code someone can scan in person, and
 * offer the same link through the share sheet for the remote case. The
 * other half — scanning / opening one of these — is ScanInviteScreen /
 * AcceptInviteScreen.
 */
export function AddFriendScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const createInvite = useCreateInvite();
  const { mutate: generateInvite } = createInvite;

  useEffect(() => {
    generateInvite();
  }, [generateInvite]);

  // Poll so the person showing the QR code sees when someone scans it — the
  // scanner's accept only writes server-side, nothing pushes to this device.
  const { friends, isSuccess } = useFriends({ pollMs: 4000 });
  const [knownIds, setKnownIds] = useState<Set<string> | null>(null);
  useEffect(() => {
    if (isSuccess) setKnownIds((prev) => prev ?? new Set(friends.map((f) => f.id)));
  }, [isSuccess, friends]);
  const newFriend = knownIds ? friends.find((f) => !knownIds.has(f.id)) : undefined;

  const inviteUrl = createInvite.data ? buildInviteUrl(createInvite.data.token) : null;

  async function share() {
    if (!inviteUrl) return;
    await Share.share({ message: `Add me on Shelf Circle: ${inviteUrl}` });
  }

  return (
    <View style={styles.container}>
      <Text style={styles.title}>Invite a friend</Text>
      <Text style={styles.subtitle}>
        Share this QR code or link — whoever opens it can add you as a friend. It expires in 7 days
        and works once.
      </Text>

      {createInvite.isPending && <ActivityIndicator color="#3b6e5e" />}
      {createInvite.isError && (
        <Text style={styles.error}>
          {createInvite.error instanceof ApiError
            ? createInvite.error.message
            : 'Could not create an invite.'}
        </Text>
      )}

      {newFriend && (
        <Text style={styles.success}>{newFriend.display_name} is now your friend!</Text>
      )}

      {inviteUrl && (
        <>
          <View style={styles.qrWrapper}>
            <QRCode value={inviteUrl} size={220} />
          </View>
          <Pressable style={styles.button} onPress={share}>
            <Text style={styles.buttonText}>Share link</Text>
          </Pressable>
        </>
      )}

      <Pressable style={styles.scanLink} onPress={() => navigation.navigate('ScanInvite')}>
        <Text style={styles.scanLinkText}>Scan a friend's QR code instead</Text>
      </Pressable>
    </View>
  );
}

const styles = StyleSheet.create({
  success: { color: '#3b6e5e', fontWeight: '600', fontSize: 16, textAlign: 'center' },
  container: { flex: 1, padding: 20, gap: 16, alignItems: 'center' },
  title: { fontSize: 20, fontWeight: '700', color: '#2b2a26', alignSelf: 'stretch' },
  subtitle: { fontSize: 14, color: '#6b6456', alignSelf: 'stretch' },
  qrWrapper: {
    padding: 20,
    backgroundColor: '#fff',
    borderRadius: 12,
    borderWidth: 1,
    borderColor: '#d9d3c4',
  },
  error: { color: '#b3432b', alignSelf: 'stretch' },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    paddingHorizontal: 32,
    alignItems: 'center',
  },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
  scanLink: { marginTop: 8 },
  scanLinkText: { color: '#3b6e5e', fontWeight: '500' },
});
