import type {
  NativeStackNavigationProp,
  NativeStackScreenProps,
} from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import React, { useState } from 'react';
import {
  ActivityIndicator,
  FlatList,
  Pressable,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';

import type { FriendSummary } from '../../friends/FriendsContext';
import { useFriends } from '../../friends/FriendsContext';
import { useCreateRecommendation } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';
import { Avatar } from '../../components/Avatar';
import { EmptyState } from '../../components/StatusViews';

type Props = NativeStackScreenProps<RootStackParamList, 'RecommendToFriend'>;

export function RecommendToFriendScreen({ route, navigation }: Props) {
  const { bookId } = route.params;
  const { friends } = useFriends();
  const [selected, setSelected] = useState<FriendSummary | null>(null);
  const [note, setNote] = useState('');
  const [sent, setSent] = useState(false);
  const createRecommendation = useCreateRecommendation();
  const rootNav = useNavigation<NativeStackNavigationProp<RootStackParamList>>();

  async function submit() {
    if (!selected || sent) return;
    setSent(true);
    try {
      await createRecommendation.mutateAsync({
        to_user_id: selected.id,
        book_id: bookId,
        note: note.trim() || undefined,
      });
      navigation.goBack();
    } catch {
      setSent(false);
    }
  }

  if (friends.length === 0) {
    return (
      <EmptyState
        title="Add a friend first to recommend books."
        action={{ label: 'Add a friend', onPress: () => rootNav.navigate('AddFriend') }}
      />
    );
  }

  return (
    <View style={styles.flex}>
      <FlatList
        data={friends}
        keyExtractor={(f) => f.id}
        contentContainerStyle={styles.list}
        renderItem={({ item }) => (
          <Pressable
            style={[styles.friendRow, selected?.id === item.id && styles.friendRowActive]}
            onPress={() => setSelected(item)}
          >
            <Avatar url={item.avatar_url} name={item.display_name} size={32} />
            <Text style={styles.friendName}>{item.display_name}</Text>
          </Pressable>
        )}
      />
      <View style={styles.footer}>
        <TextInput
          style={styles.note}
          placeholder="Add a note (optional)"
          value={note}
          onChangeText={setNote}
          maxLength={280}
          multiline
        />
        <Pressable
          style={[styles.button, (!selected || sent) && styles.buttonDisabled]}
          onPress={submit}
          disabled={!selected || sent}
        >
          {sent ? (
            <ActivityIndicator color="#fff" />
          ) : (
            <Text style={styles.buttonText}>Send recommendation</Text>
          )}
        </Pressable>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  list: { padding: 16, gap: 4 },
  friendRow: {
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    padding: 10,
    borderRadius: 8,
  },
  friendRowActive: { backgroundColor: '#e6efe9' },
  friendName: { fontSize: 15, color: '#2b2a26', fontWeight: '500' },
  footer: { padding: 16, gap: 12, borderTopWidth: 1, borderTopColor: '#efeae0' },
  note: {
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 10,
    fontSize: 15,
    backgroundColor: '#fff',
    minHeight: 44,
  },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
  },
  buttonDisabled: { opacity: 0.5 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
});
