import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { useNavigation } from '@react-navigation/native';
import * as Crypto from 'expo-crypto';
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

import { resolveBookByReference, resolveManualBook } from '../../api/endpoints';
import { BookSearchResults } from '../../components/BookSearchResults';
import type { RootStackParamList } from '../../navigation/types';

export function AddPastReadScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const [manual, setManual] = useState(false);
  const [resolving, setResolving] = useState(false);

  async function goToFinish(bookId: string) {
    navigation.replace('FinishBook', { bookId, initialStatus: 'finished' });
  }

  async function onSelect(result: { source: string; source_id: string }) {
    if (resolving) return;
    setResolving(true);
    try {
      const resolved = await resolveBookByReference(result.source, result.source_id);
      await goToFinish(resolved.id);
    } finally {
      setResolving(false);
    }
  }

  if (manual) {
    return <ManualEntryForm onResolved={goToFinish} onCancel={() => setManual(false)} busy={resolving} />;
  }

  return (
    <View style={styles.flex}>
      <BookSearchResults onSelect={onSelect} />
      <Pressable style={styles.manualLink} onPress={() => setManual(true)}>
        <Text style={styles.manualLinkText}>Can't find it? Add manually</Text>
      </Pressable>
      {resolving && (
        <View style={styles.overlay}>
          <ActivityIndicator size="large" color="#3b6e5e" />
        </View>
      )}
    </View>
  );
}

function ManualEntryForm({
  onResolved,
  onCancel,
  busy,
}: {
  onResolved: (bookId: string) => void;
  onCancel: () => void;
  busy: boolean;
}) {
  const [title, setTitle] = useState('');
  const [author, setAuthor] = useState('');
  const [language, setLanguage] = useState('en');
  const [publisher, setPublisher] = useState('');
  const [isbn13, setIsbn13] = useState('');
  const [isbn10, setIsbn10] = useState('');
  const [submitting, setSubmitting] = useState(false);

  async function submit() {
    if (!title.trim()) return;
    setSubmitting(true);
    try {
      const resolved = await resolveManualBook({
        canonical_title: title.trim(),
        primary_author: author.trim() || null,
        language,
        isbn_13: isbn13.trim() || null,
        isbn_10: isbn10.trim() || null,
        edition_title: title.trim(),
        publisher: publisher.trim() || null,
        cover_image_url: null,
        source: 'manual',
        source_id: `manual:${Crypto.randomUUID()}`,
        open_library_work_id: null,
        google_books_volume_id: null,
      });
      onResolved(resolved.id);
    } finally {
      setSubmitting(false);
    }
  }

  const disabled = busy || submitting || title.trim().length === 0;

  return (
    <KeyboardAvoidingView style={styles.flex} behavior={Platform.OS === 'ios' ? 'padding' : undefined}>
      <ScrollView contentContainerStyle={styles.form} keyboardShouldPersistTaps="handled">
        <Field label="Title" value={title} onChangeText={setTitle} required />
        <Field label="Author" value={author} onChangeText={setAuthor} />
        <Field label="Language (BCP-47)" value={language} onChangeText={setLanguage} autoCapitalize="none" />
        <Field label="Publisher" value={publisher} onChangeText={setPublisher} />
        <Field label="ISBN-13" value={isbn13} onChangeText={setIsbn13} autoCapitalize="none" />
        <Field label="ISBN-10" value={isbn10} onChangeText={setIsbn10} autoCapitalize="none" />

        <Pressable style={[styles.button, disabled && styles.buttonDisabled]} onPress={submit} disabled={disabled}>
          {submitting ? <ActivityIndicator color="#fff" /> : <Text style={styles.buttonText}>Continue</Text>}
        </Pressable>
        <Pressable onPress={onCancel} disabled={submitting}>
          <Text style={styles.manualLinkText}>Back to search</Text>
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

function Field({
  label,
  value,
  onChangeText,
  required,
  autoCapitalize,
}: {
  label: string;
  value: string;
  onChangeText: (t: string) => void;
  required?: boolean;
  autoCapitalize?: 'none' | 'words';
}) {
  return (
    <View style={styles.field}>
      <Text style={styles.label}>
        {label}
        {required ? ' *' : ''}
      </Text>
      <TextInput
        style={styles.input}
        value={value}
        onChangeText={onChangeText}
        autoCapitalize={autoCapitalize ?? 'sentences'}
        autoCorrect={autoCapitalize === 'none' ? false : undefined}
      />
    </View>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  manualLink: { padding: 16, alignItems: 'center' },
  manualLinkText: { color: '#3b6e5e', fontWeight: '600' },
  overlay: {
    position: 'absolute',
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    backgroundColor: 'rgba(250,248,243,0.7)',
    alignItems: 'center',
    justifyContent: 'center',
  },
  form: { padding: 20, gap: 16 },
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
  },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
    marginTop: 8,
  },
  buttonDisabled: { opacity: 0.5 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
});
