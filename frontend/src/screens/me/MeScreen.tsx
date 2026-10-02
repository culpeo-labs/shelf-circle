import { useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import React from 'react';
import { Pressable, ScrollView, StyleSheet, Switch, Text, View } from 'react-native';

import { useAuth } from '../../auth/AuthContext';
import { Avatar } from '../../components/Avatar';
import { ReadingYearCard } from '../../components/ReadingYearCard';
import { useMyLibrarySystem, useUpdateMe } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

export function MeScreen() {
  const { user, signOut } = useAuth();
  const updateMe = useUpdateMe();
  const myLibrary = useMyLibrarySystem();
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  if (!user) return null;

  return (
    <ScrollView contentContainerStyle={styles.container}>
      <Avatar url={user.avatar_url} name={user.display_name} size={80} />
      <Text style={styles.name}>{user.display_name}</Text>
      <Text style={styles.handle}>@{user.handle}</Text>
      <Pressable onPress={() => navigation.navigate('EditProfile')} style={styles.editButton}>
        <Text style={styles.editText}>Edit profile</Text>
      </Pressable>

      <ReadingYearCard />

      <View style={styles.section}>
        <Pressable onPress={() => navigation.navigate('ChooseLibrary')}>
          <Row
            label="My library"
            value={`${myLibrary.data?.library_system?.name ?? 'Not set'}  ›`}
          />
        </Pressable>
        <Row label="Locale" value={user.locale} />
        <Row label="Member since" value={new Date(user.created_at).toLocaleDateString()} />
      </View>

      <View style={styles.shareRow}>
        <View style={styles.shareText}>
          <Text style={styles.shareTitle}>Share my bookshelves with friends</Text>
          <Text style={styles.note}>
            {user.share_shelves
              ? 'Your friends can see what you’re reading, have read, and want to read.'
              : 'Only you can see your bookshelves. Your timeline activity is unaffected.'}
          </Text>
          {updateMe.isError && <Text style={styles.error}>Couldn’t save that. Try again.</Text>}
        </View>
        <Switch
          value={user.share_shelves}
          disabled={updateMe.isPending}
          onValueChange={(value) => updateMe.mutate({ share_shelves: value })}
          trackColor={{ true: '#3b6e5e' }}
        />
      </View>

      <Pressable style={styles.signOutButton} onPress={() => void signOut()}>
        <Text style={styles.signOutText}>Sign out</Text>
      </Pressable>

      <Pressable onPress={() => navigation.navigate('DeleteAccount')} style={styles.deleteLink}>
        <Text style={styles.deleteLinkText}>Delete account</Text>
      </Pressable>
    </ScrollView>
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
  container: { flexGrow: 1, alignItems: 'center', padding: 32, paddingTop: 48, gap: 4 },
  name: { fontSize: 22, fontWeight: '700', color: '#2b2a26', marginTop: 12 },
  handle: { fontSize: 14, color: '#6b6456' },
  editButton: { marginTop: 12, paddingVertical: 6, paddingHorizontal: 16 },
  editText: { color: '#3b6e5e', fontWeight: '600', fontSize: 15 },
  section: { alignSelf: 'stretch', marginTop: 32, gap: 4 },
  row: { flexDirection: 'row', justifyContent: 'space-between', paddingVertical: 8 },
  rowLabel: { color: '#6b6456' },
  rowValue: { color: '#2b2a26', fontWeight: '500' },
  note: { fontSize: 12, color: '#918a78', marginTop: 4 },
  error: { fontSize: 12, color: '#b3432b', marginTop: 4 },
  shareRow: {
    alignSelf: 'stretch',
    flexDirection: 'row',
    alignItems: 'center',
    gap: 12,
    marginTop: 24,
  },
  shareText: { flex: 1 },
  shareTitle: { fontSize: 15, fontWeight: '600', color: '#2b2a26' },
  signOutButton: {
    marginTop: 40,
    borderWidth: 1,
    borderColor: '#b3432b',
    borderRadius: 8,
    paddingVertical: 12,
    paddingHorizontal: 24,
  },
  signOutText: { color: '#b3432b', fontWeight: '600' },
  deleteLink: { marginTop: 16, paddingVertical: 8, paddingHorizontal: 12 },
  deleteLinkText: { color: '#6b6456', fontSize: 13, textDecorationLine: 'underline' },
});
