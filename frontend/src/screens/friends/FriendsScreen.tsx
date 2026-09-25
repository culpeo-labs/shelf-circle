import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useFocusEffect, useNavigation } from '@react-navigation/native';
import React, { useCallback } from 'react';
import { FlatList, Pressable, RefreshControl, StyleSheet, Text, View } from 'react-native';

import { Avatar } from '../../components/Avatar';
import { EmptyState } from '../../components/StatusViews';
import {
  useApproveFriendRequest,
  useDeclineFriendRequest,
  useFriendRequests,
  useFriends,
} from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

export function FriendsScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { friends, refetch, isRefetching } = useFriends();

  // Someone may have accepted your invite while you were elsewhere.
  useFocusEffect(
    useCallback(() => {
      void refetch();
    }, [refetch]),
  );

  return (
    <View style={styles.flex}>
      <Pressable style={styles.addButton} onPress={() => navigation.navigate('AddFriend')}>
        <Text style={styles.addButtonText}>+ Add friend</Text>
      </Pressable>

      <FriendRequests />

      {friends.length === 0 ? (
        <EmptyState
          title="Add a friend to get started."
          action={{ label: 'Add a friend', onPress: () => navigation.navigate('AddFriend') }}
        />
      ) : (
        <FlatList
          data={friends}
          keyExtractor={(f) => f.id}
          contentContainerStyle={styles.list}
          refreshControl={
            <RefreshControl refreshing={isRefetching} onRefresh={() => void refetch()} />
          }
          renderItem={({ item }) => (
            <Pressable
              style={styles.row}
              onPress={() =>
                navigation.navigate('FriendProfile', {
                  userId: item.id,
                  displayName: item.display_name,
                })
              }
            >
              <Avatar url={item.avatar_url} name={item.display_name} />
              <View>
                <Text style={styles.name}>{item.display_name}</Text>
                <Text style={styles.handle}>@{item.handle}</Text>
              </View>
            </Pressable>
          )}
        />
      )}
    </View>
  );
}

/** People who used one of your reusable invite links and are waiting for you. */
function FriendRequests() {
  const requests = useFriendRequests();
  const approve = useApproveFriendRequest();
  const decline = useDeclineFriendRequest();
  const pending = requests.data ?? [];
  if (pending.length === 0) return null;
  const busy = approve.isPending || decline.isPending;

  return (
    <View style={styles.requests}>
      <Text style={styles.requestsTitle}>Friend requests</Text>
      {pending.map((r) => (
        <View key={r.id} style={styles.requestRow}>
          <Avatar url={r.avatar_url} name={r.display_name} />
          <Text style={[styles.name, styles.flex]} numberOfLines={1}>
            {r.display_name}
          </Text>
          <Pressable onPress={() => decline.mutate(r.id)} disabled={busy} hitSlop={8}>
            <Text style={styles.declineText}>Decline</Text>
          </Pressable>
          <Pressable
            style={[styles.approve, busy && styles.approveDisabled]}
            onPress={() => approve.mutate(r.id)}
            disabled={busy}
          >
            <Text style={styles.approveText}>Approve</Text>
          </Pressable>
        </View>
      ))}
      {(approve.isError || decline.isError) && (
        <Text style={styles.requestError}>Couldn’t save that. Try again.</Text>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  requests: {
    marginHorizontal: 16,
    marginTop: 12,
    padding: 12,
    gap: 10,
    borderRadius: 12,
    backgroundColor: '#e6efe9',
  },
  requestsTitle: { fontSize: 13, fontWeight: '700', color: '#3b6e5e', textTransform: 'uppercase' },
  requestRow: { flexDirection: 'row', alignItems: 'center', gap: 10 },
  declineText: { color: '#6b6456', fontSize: 14 },
  approve: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 8,
    paddingHorizontal: 14,
  },
  approveDisabled: { opacity: 0.6 },
  approveText: { color: '#fff', fontWeight: '600', fontSize: 14 },
  requestError: { color: '#b3432b', fontSize: 12 },
  flex: { flex: 1 },
  addButton: { alignSelf: 'flex-start', margin: 16, marginBottom: 0 },
  addButtonText: { color: '#3b6e5e', fontWeight: '600', fontSize: 15 },
  list: { padding: 16, gap: 16 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  name: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  handle: { fontSize: 13, color: '#6b6456' },
});
