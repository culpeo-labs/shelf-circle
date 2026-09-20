import React from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';

import { useAuth } from '../../auth/AuthContext';
import { Avatar } from '../../components/Avatar';

export function MeScreen() {
  const { user, signOut } = useAuth();
  if (!user) return null;

  return (
    <View style={styles.container}>
      <Avatar url={user.avatar_url} name={user.display_name} size={80} />
      <Text style={styles.name}>{user.display_name}</Text>
      <Text style={styles.handle}>@{user.handle}</Text>

      <View style={styles.section}>
        <Row label="Locale" value={user.locale} />
        <Row label="Member since" value={new Date(user.created_at).toLocaleDateString()} />
      </View>

      <Text style={styles.note}>Profile editing coming soon.</Text>

      <Pressable style={styles.signOutButton} onPress={() => void signOut()}>
        <Text style={styles.signOutText}>Sign out</Text>
      </Pressable>
    </View>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <View style={styles.row}>
      <Text style={styles.rowLabel}>{label}</Text>
      <Text style={styles.rowValue}>{value}</Text>
    </View>
  );
}

const styles = StyleSheet.create({
  container: { flex: 1, alignItems: 'center', padding: 32, paddingTop: 48, gap: 4 },
  name: { fontSize: 22, fontWeight: '700', color: '#2b2a26', marginTop: 12 },
  handle: { fontSize: 14, color: '#6b6456' },
  section: { alignSelf: 'stretch', marginTop: 32, gap: 4 },
  row: { flexDirection: 'row', justifyContent: 'space-between', paddingVertical: 8 },
  rowLabel: { color: '#6b6456' },
  rowValue: { color: '#2b2a26', fontWeight: '500' },
  note: { fontSize: 12, color: '#918a78', marginTop: 16 },
  signOutButton: {
    marginTop: 40,
    borderWidth: 1,
    borderColor: '#b3432b',
    borderRadius: 8,
    paddingVertical: 12,
    paddingHorizontal: 24,
  },
  signOutText: { color: '#b3432b', fontWeight: '600' },
});
