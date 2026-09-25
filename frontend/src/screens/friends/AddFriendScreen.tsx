import { useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import React, { useEffect, useState } from 'react';
import {
  ActivityIndicator,
  Pressable,
  ScrollView,
  Share,
  StyleSheet,
  Text,
  View,
} from 'react-native';
import QRCode from 'react-native-qrcode-svg';

import { ApiError } from '../../api/client';
import { useCreateInvite, useFriends, useMyInvites, useRevokeInvite } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';
import { buildInviteUrl } from '../../utils/inviteLink';

/**
 * "Add a friend" is invite-based (see friends.md): make a QR code / link
 * someone opens to connect. Two modes:
 *  - **One person** (default): a single-use link that connects whoever opens
 *    it first straight away — you chose who to hand it to.
 *  - **Anyone with the link**: a reusable link (30 days) for sharing more
 *    widely; each person who uses it becomes a request you approve on the
 *    Friends tab, and you can stop the link at any time.
 */
export function AddFriendScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const [reusable, setReusable] = useState(false);
  const createInvite = useCreateInvite();
  const revoke = useRevokeInvite();
  const myInvites = useMyInvites();
  const { mutate: generateInvite, reset: resetCreate } = createInvite;

  const activeReusable = (myInvites.data ?? []).filter((i) => i.reusable);
  const shownReusable = activeReusable[0];

  // One-person mode: a fresh single-use invite each time it's chosen.
  useEffect(() => {
    resetCreate();
    if (!reusable) generateInvite(false);
  }, [reusable, generateInvite, resetCreate]);

  // Anyone-with-the-link mode: reuse the active link if there is one (so
  // reopening this screen doesn't pile up live links); create one only if none.
  const needsReusable = reusable && myInvites.isSuccess && !shownReusable;
  useEffect(() => {
    if (needsReusable && !createInvite.isPending && !createInvite.data) generateInvite(true);
  }, [needsReusable, createInvite.isPending, createInvite.data, generateInvite]);

  // Poll so the person showing the QR code sees when a one-person invite is
  // used — the scanner's accept only writes server-side, nothing pushes here.
  const { friends, isSuccess } = useFriends({ pollMs: 4000 });
  const [knownIds, setKnownIds] = useState<Set<string> | null>(null);
  useEffect(() => {
    if (isSuccess) setKnownIds((prev) => prev ?? new Set(friends.map((f) => f.id)));
  }, [isSuccess, friends]);
  const newFriend = knownIds ? friends.find((f) => !knownIds.has(f.id)) : undefined;

  const token = reusable ? shownReusable?.token : createInvite.data?.token;
  const inviteUrl = token ? buildInviteUrl(token) : null;

  async function share() {
    if (!inviteUrl) return;
    await Share.share({ message: `Add me on Shelf Circle: ${inviteUrl}` });
  }

  return (
    <ScrollView contentContainerStyle={styles.container}>
      <Text style={styles.title}>Invite a friend</Text>

      <View style={styles.segmented}>
        {[
          { key: false, label: 'One person' },
          { key: true, label: 'Anyone with the link' },
        ].map((m) => (
          <Pressable
            key={String(m.key)}
            style={[styles.segment, reusable === m.key && styles.segmentActive]}
            onPress={() => setReusable(m.key)}
          >
            <Text style={[styles.segmentText, reusable === m.key && styles.segmentTextActive]}>
              {m.label}
            </Text>
          </Pressable>
        ))}
      </View>

      <Text style={styles.subtitle}>
        {reusable
          ? 'Share this QR code or link with several people. Each one who uses it sends you a request, and you approve who becomes a friend. It works for 30 days, or until you stop it.'
          : 'Share this QR code or link with one person — they become your friend right away. It works once and expires in 7 days.'}
      </Text>

      {(createInvite.isPending || (reusable && myInvites.isLoading)) && (
        <ActivityIndicator color="#3b6e5e" />
      )}
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

      {reusable && myInvites.isSuccess && !shownReusable && !createInvite.isPending && (
        <Pressable style={styles.button} onPress={() => generateInvite(true)}>
          <Text style={styles.buttonText}>Create a new link</Text>
        </Pressable>
      )}

      {reusable && activeReusable.length > 0 && (
        <View style={styles.linkList}>
          <Text style={styles.listTitle}>Your active links</Text>
          {activeReusable.map((invite) => (
            <View key={invite.token} style={styles.linkRow}>
              <View style={styles.flex}>
                <Text style={styles.linkMain}>
                  {invite.use_count} joined
                  {invite.pending_requests > 0 ? ` · ${invite.pending_requests} waiting` : ''}
                </Text>
                <Text style={styles.linkSub}>
                  Expires {new Date(invite.expires_at).toLocaleDateString()}
                </Text>
              </View>
              <Pressable
                onPress={() => revoke.mutate(invite.token)}
                disabled={revoke.isPending}
                hitSlop={8}
              >
                <Text style={styles.stop}>Stop link</Text>
              </Pressable>
            </View>
          ))}
        </View>
      )}

      <Pressable style={styles.scanLink} onPress={() => navigation.navigate('ScanInvite')}>
        <Text style={styles.scanLinkText}>Scan a friend's QR code instead</Text>
      </Pressable>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  success: { color: '#3b6e5e', fontWeight: '600', fontSize: 16, textAlign: 'center' },
  container: { padding: 20, gap: 16, alignItems: 'center' },
  title: { fontSize: 20, fontWeight: '700', color: '#2b2a26', alignSelf: 'stretch' },
  subtitle: { fontSize: 14, color: '#6b6456', alignSelf: 'stretch' },
  segmented: {
    flexDirection: 'row',
    alignSelf: 'stretch',
    backgroundColor: '#efeae0',
    borderRadius: 8,
    padding: 3,
  },
  segment: { flex: 1, paddingVertical: 8, borderRadius: 6, alignItems: 'center' },
  segmentActive: { backgroundColor: '#fff' },
  segmentText: { fontSize: 13, color: '#6b6456', fontWeight: '500' },
  segmentTextActive: { color: '#2b2a26', fontWeight: '600' },
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
  linkList: { alignSelf: 'stretch', gap: 8, marginTop: 8 },
  listTitle: { fontSize: 13, fontWeight: '600', color: '#6b6456', textTransform: 'uppercase' },
  linkRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    padding: 12,
    borderRadius: 8,
    borderWidth: 1,
    borderColor: '#e5e1d8',
    backgroundColor: '#fff',
  },
  linkMain: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  linkSub: { fontSize: 12, color: '#918a78' },
  stop: { color: '#b3432b', fontWeight: '600' },
  scanLink: { marginTop: 8 },
  scanLinkText: { color: '#3b6e5e', fontWeight: '500' },
});
