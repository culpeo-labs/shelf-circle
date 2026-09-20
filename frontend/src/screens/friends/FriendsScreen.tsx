import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import React from 'react';
import { FlatList, Pressable, StyleSheet, Text, View } from 'react-native';

import { Avatar } from '../../components/Avatar';
import { EmptyState } from '../../components/StatusViews';
import { useFriends } from '../../friends/FriendsContext';
import type { RootStackParamList } from '../../navigation/types';

export function FriendsScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const { friends } = useFriends();

  return (
    <View style={styles.flex}>
      <Pressable style={styles.addButton} onPress={() => navigation.navigate('AddFriend')}>
        <Text style={styles.addButtonText}>+ Add friend</Text>
      </Pressable>

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
          renderItem={({ item }) => (
            <Pressable
              style={styles.row}
              onPress={() => navigation.navigate('FriendProfile', { userId: item.id, displayName: item.display_name })}
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

const styles = StyleSheet.create({
  flex: { flex: 1 },
  addButton: { alignSelf: 'flex-start', margin: 16, marginBottom: 0 },
  addButtonText: { color: '#3b6e5e', fontWeight: '600', fontSize: 15 },
  list: { padding: 16, gap: 16 },
  row: { flexDirection: 'row', alignItems: 'center', gap: 12 },
  name: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  handle: { fontSize: 13, color: '#6b6456' },
});
