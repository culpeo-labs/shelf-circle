import React from 'react';
import {
  ActivityIndicator,
  Alert,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from 'react-native';

import { ApiError } from '../../api/client';
import { useDeleteAccount } from '../../hooks/queries';

const WHAT_GOES = [
  'Your profile and profile photo',
  'Your bookshelves, ratings, reading history, and yearly goals',
  'Your friends, invites, and the recommendations you’ve sent or received',
  'Your sign-in account (your email address)',
];

/** In-app account deletion (also a Google Play requirement). Two steps on purpose:
 * this explanation screen, then a confirmation dialog. */
export function DeleteAccountScreen() {
  const deleteAccount = useDeleteAccount();

  function confirm() {
    Alert.alert(
      'Delete your account?',
      'Everything listed here will be permanently deleted. This can’t be undone.',
      [
        { text: 'Cancel', style: 'cancel' },
        { text: 'Delete', style: 'destructive', onPress: () => deleteAccount.mutate() },
      ],
    );
  }

  return (
    <ScrollView contentContainerStyle={styles.container}>
      <Text style={styles.title}>Delete your account</Text>
      <Text style={styles.body}>
        This permanently deletes your Shelf Circle account and everything in it:
      </Text>
      <View style={styles.list}>
        {WHAT_GOES.map((item) => (
          <Text key={item} style={styles.item}>
            • {item}
          </Text>
        ))}
      </View>
      <Text style={styles.body}>
        Your friends will no longer see anything from you. This can’t be undone, and you’ll be
        signed out. You can create a new account with the same email later, but it will start empty.
      </Text>

      {deleteAccount.isError && (
        <Text style={styles.error}>
          {deleteAccount.error instanceof ApiError
            ? deleteAccount.error.message
            : 'Something went wrong. Nothing was deleted — please try again.'}
        </Text>
      )}

      <Pressable
        style={[styles.button, deleteAccount.isPending && styles.buttonDisabled]}
        onPress={confirm}
        disabled={deleteAccount.isPending}
      >
        {deleteAccount.isPending ? (
          <ActivityIndicator color="#fff" />
        ) : (
          <Text style={styles.buttonText}>Delete my account</Text>
        )}
      </Pressable>
    </ScrollView>
  );
}

const styles = StyleSheet.create({
  container: { padding: 24, gap: 16 },
  title: { fontSize: 22, fontWeight: '700', color: '#2b2a26' },
  body: { fontSize: 15, color: '#2b2a26', lineHeight: 22 },
  list: { gap: 6, paddingLeft: 4 },
  item: { fontSize: 15, color: '#2b2a26', lineHeight: 22 },
  error: { color: '#b3432b' },
  button: {
    backgroundColor: '#b3432b',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
    marginTop: 8,
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
});
