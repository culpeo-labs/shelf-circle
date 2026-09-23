import React, { useState } from 'react';
import {
  ActivityIndicator,
  KeyboardAvoidingView,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  TextInput,
  View,
} from 'react-native';

import { ApiError } from '../../api/client';
import { createUser } from '../../api/endpoints';
import { useAuth } from '../../auth/AuthContext';

const HANDLE_PATTERN = /^[a-z0-9_]+$/;

export function CreateIdentityScreen() {
  const { completeOnboarding, signOut } = useAuth();
  const [displayName, setDisplayName] = useState('');
  const [handle, setHandle] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleError =
    handle.length > 0 && !HANDLE_PATTERN.test(handle)
      ? 'Lowercase letters, numbers, and underscores only.'
      : null;

  const canSubmit = displayName.trim().length > 0 && handle.length > 0 && !handleError && !busy;

  async function submit() {
    if (!canSubmit) return;
    setBusy(true);
    setError(null);
    try {
      const user = await createUser({ handle, display_name: displayName.trim() });
      await completeOnboarding(user);
    } catch (e) {
      if (e instanceof ApiError && e.status === 409) {
        setError('That handle may be taken — try another.');
      } else {
        setError(e instanceof Error ? e.message : 'Something went wrong.');
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <KeyboardAvoidingView
      style={styles.flex}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <ScrollView contentContainerStyle={styles.container} keyboardShouldPersistTaps="handled">
        <Text style={styles.title}>Welcome</Text>
        <Text style={styles.subtitle}>Set up your profile to get started.</Text>

        {error && <Text style={styles.error}>{error}</Text>}

        <View style={styles.field}>
          <Text style={styles.label}>Display name</Text>
          <TextInput
            placeholderTextColor="#918a78"
            style={styles.input}
            value={displayName}
            onChangeText={setDisplayName}
            placeholder="Ada Lovelace"
            autoCapitalize="words"
            editable={!busy}
          />
        </View>

        <View style={styles.field}>
          <Text style={styles.label}>Handle</Text>
          <TextInput
            placeholderTextColor="#918a78"
            style={styles.input}
            value={handle}
            onChangeText={(text) => setHandle(text.toLowerCase())}
            placeholder="ada"
            autoCapitalize="none"
            autoCorrect={false}
            editable={!busy}
          />
          {handleError && <Text style={styles.fieldError}>{handleError}</Text>}
        </View>

        <Pressable
          style={[styles.button, !canSubmit && styles.buttonDisabled]}
          onPress={submit}
          disabled={!canSubmit}
        >
          {busy ? (
            <ActivityIndicator color="#fff" />
          ) : (
            <Text style={styles.buttonText}>Continue</Text>
          )}
        </Pressable>

        <Pressable onPress={() => void signOut()} disabled={busy}>
          <Text style={styles.link}>Use a different email</Text>
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1, backgroundColor: '#faf8f3' },
  container: { flexGrow: 1, justifyContent: 'center', padding: 24, gap: 16 },
  title: { fontSize: 28, fontWeight: '700', color: '#2b2a26', textAlign: 'center' },
  subtitle: { fontSize: 15, color: '#6b6456', textAlign: 'center', marginBottom: 8 },
  field: { gap: 6 },
  label: { fontSize: 13, fontWeight: '600', color: '#6b6456' },
  input: {
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 8,
    paddingHorizontal: 14,
    paddingVertical: 12,
    fontSize: 16,
    backgroundColor: '#fff',
    color: '#2b2a26',
  },
  fieldError: { color: '#b3432b', fontSize: 13 },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
    marginTop: 8,
  },
  buttonDisabled: { opacity: 0.5 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
  link: { color: '#3b6e5e', fontWeight: '500', textAlign: 'center', marginTop: 4 },
  error: { color: '#b3432b', textAlign: 'center' },
});
