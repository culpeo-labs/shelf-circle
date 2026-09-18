import type { NativeStackScreenProps } from '@react-navigation/native-stack';
import React from 'react';
import { StyleSheet, Text, View } from 'react-native';

import { Avatar } from '../../components/Avatar';
import { useUser } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'FriendProfile'>;

export function FriendProfileScreen({ route }: Props) {
  const { userId, displayName } = route.params;
  const { data } = useUser(userId);

  return (
    <View style={styles.container}>
      <Avatar url={data?.avatar_url ?? null} name={data?.display_name ?? displayName} size={72} />
      <Text style={styles.name}>{data?.display_name ?? displayName}</Text>
      {data?.handle && <Text style={styles.handle}>@{data.handle}</Text>}

      <Text style={styles.note}>
        Their shelves aren't visible here yet — the backend only exposes a user's reading
        library to themselves right now.
      </Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, alignItems: 'center', padding: 32, gap: 6, paddingTop: 48 },
  name: { fontSize: 20, fontWeight: '700', color: '#2b2a26', marginTop: 8 },
  handle: { fontSize: 14, color: '#6b6456' },
  note: { fontSize: 13, color: '#918a78', textAlign: 'center', marginTop: 24, lineHeight: 19 },
});
