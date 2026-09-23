import type { NativeStackScreenProps } from '@react-navigation/native-stack';
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
import { useAuth } from '../../auth/AuthContext';
import { Avatar } from '../../components/Avatar';
import { useUpdateMe } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';
import { pickAndUploadAvatar } from '../../utils/avatar';

type Props = NativeStackScreenProps<RootStackParamList, 'EditProfile'>;

const MAX_NAME_LENGTH = 50;

export function EditProfileScreen({ navigation }: Props) {
  const { user } = useAuth();
  const updateMe = useUpdateMe();
  const [displayName, setDisplayName] = useState(user?.display_name ?? '');
  // `undefined` = unchanged, `null` = removed, string = newly uploaded URL.
  const [avatarUrl, setAvatarUrl] = useState<string | null | undefined>(undefined);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!user) return null;

  const shownAvatar = avatarUrl === undefined ? user.avatar_url : avatarUrl;
  const trimmed = displayName.trim();
  const nameChanged = trimmed !== user.display_name;
  const avatarChanged = avatarUrl !== undefined;
  const canSave =
    trimmed.length > 0 && (nameChanged || avatarChanged) && !uploading && !updateMe.isPending;

  async function changePhoto() {
    setError(null);
    setUploading(true);
    try {
      const picked = await pickAndUploadAvatar();
      if (picked) setAvatarUrl(picked.avatarUrl);
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not update the photo.');
    } finally {
      setUploading(false);
    }
  }

  async function save() {
    if (!canSave) return;
    setError(null);
    try {
      await updateMe.mutateAsync({
        ...(nameChanged ? { display_name: trimmed } : {}),
        ...(avatarChanged ? { avatar_url: avatarUrl } : {}),
      });
      navigation.goBack();
    } catch (e) {
      setError(e instanceof ApiError ? e.message : 'Could not save your profile.');
    }
  }

  return (
    <KeyboardAvoidingView
      style={styles.flex}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <ScrollView contentContainerStyle={styles.container} keyboardShouldPersistTaps="handled">
        <Pressable onPress={changePhoto} disabled={uploading} style={styles.avatarWrap}>
          <Avatar url={shownAvatar} name={trimmed || user.display_name} size={104} />
          {uploading && (
            <View style={styles.avatarOverlay}>
              <ActivityIndicator color="#fff" />
            </View>
          )}
        </Pressable>
        <Pressable onPress={changePhoto} disabled={uploading}>
          <Text style={styles.link}>{shownAvatar ? 'Change photo' : 'Add a photo'}</Text>
        </Pressable>
        {shownAvatar && (
          <Pressable onPress={() => setAvatarUrl(null)} disabled={uploading}>
            <Text style={styles.removeLink}>Remove photo</Text>
          </Pressable>
        )}

        {error && <Text style={styles.error}>{error}</Text>}

        <View style={styles.field}>
          <Text style={styles.label}>Display name</Text>
          <TextInput
            style={styles.input}
            placeholderTextColor="#918a78"
            value={displayName}
            onChangeText={setDisplayName}
            maxLength={MAX_NAME_LENGTH}
            autoCapitalize="words"
            editable={!updateMe.isPending}
          />
        </View>
        <Text style={styles.hint}>Your handle (@{user.handle}) can't be changed.</Text>

        <Pressable
          style={[styles.button, !canSave && styles.buttonDisabled]}
          onPress={save}
          disabled={!canSave}
        >
          {updateMe.isPending ? (
            <ActivityIndicator color="#fff" />
          ) : (
            <Text style={styles.buttonText}>Save</Text>
          )}
        </Pressable>
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1, backgroundColor: '#faf8f3' },
  container: { alignItems: 'center', padding: 24, gap: 12 },
  avatarWrap: { marginTop: 12 },
  avatarOverlay: {
    ...StyleSheet.absoluteFill,
    borderRadius: 52,
    backgroundColor: 'rgba(0,0,0,0.4)',
    alignItems: 'center',
    justifyContent: 'center',
  },
  link: { color: '#3b6e5e', fontWeight: '600' },
  removeLink: { color: '#b3432b', fontSize: 13 },
  error: { color: '#b3432b', textAlign: 'center' },
  field: { alignSelf: 'stretch', gap: 6, marginTop: 16 },
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
  hint: { alignSelf: 'stretch', fontSize: 12, color: '#918a78' },
  button: {
    alignSelf: 'stretch',
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 14,
    alignItems: 'center',
    marginTop: 12,
  },
  buttonDisabled: { opacity: 0.5 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
});
