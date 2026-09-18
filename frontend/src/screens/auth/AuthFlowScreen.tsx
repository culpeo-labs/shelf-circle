import React, { useEffect, useState } from 'react';
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

import { startFlow, submitFlowAction, type FlowAction, type FlowResult } from '../../auth/hankoFlowClient';
import { useAuth } from '../../auth/AuthContext';

/**
 * Renders whatever the Hanko Flow API currently asks for — an email field,
 * then a passcode field, in the passcode setup this app was built against —
 * without hardcoding step names. See `hankoFlowClient.ts` for why.
 */
export function AuthFlowScreen() {
  const { completeWithToken } = useAuth();
  const [state, setState] = useState<FlowResult | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void restart();
  }, []);

  async function restart() {
    setBusy(true);
    setError(null);
    try {
      await applyResult(await startFlow('/login'));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not start sign-in.');
    } finally {
      setBusy(false);
    }
  }

  async function applyResult(result: FlowResult) {
    if (result.authToken) {
      await completeWithToken(result.authToken);
      return;
    }
    setState(result);
    setValues({});
    setError(result.error?.message ?? null);
  }

  async function runAction(actionName: string, action: FlowAction) {
    if (!state) return;
    setBusy(true);
    setError(null);
    try {
      const inputData: Record<string, string> = {};
      for (const key of Object.keys(action.inputs)) {
        if (values[key] !== undefined) inputData[key] = values[key];
      }
      await applyResult(await submitFlowAction(state, actionName, inputData));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Something went wrong.');
    } finally {
      setBusy(false);
    }
  }

  if (busy && !state) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator size="large" color="#3b6e5e" />
      </View>
    );
  }

  if (!state) {
    return (
      <View style={styles.centered}>
        <Text style={styles.error}>{error ?? 'Could not start sign-in.'}</Text>
        <Pressable style={styles.secondaryButton} onPress={restart}>
          <Text style={styles.secondaryButtonText}>Try again</Text>
        </Pressable>
      </View>
    );
  }

  const actionEntries = Object.entries(state.actions);
  const withInputs = actionEntries.filter(([, a]) =>
    Object.values(a.inputs).some((i) => !i.hidden)
  );
  const withoutInputs = actionEntries.filter(
    ([, a]) => !Object.values(a.inputs).some((i) => !i.hidden)
  );

  return (
    <KeyboardAvoidingView
      style={styles.flex}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      <ScrollView contentContainerStyle={styles.container} keyboardShouldPersistTaps="handled">
        <Text style={styles.title}>Leecommend</Text>
        <Text style={styles.subtitle}>Sign in with your email to continue.</Text>

        {error && <Text style={styles.error}>{error}</Text>}

        {withInputs.map(([name, action]) => (
          <View key={name} style={styles.card}>
            {Object.values(action.inputs)
              .filter((input) => !input.hidden)
              .map((input) => (
                <TextInput
                  key={input.name}
                  style={styles.input}
                  placeholder={prettify(input.name)}
                  value={values[input.name] ?? ''}
                  onChangeText={(text) => setValues((v) => ({ ...v, [input.name]: text }))}
                  autoCapitalize="none"
                  autoCorrect={false}
                  keyboardType={input.name.includes('email') ? 'email-address' : 'default'}
                  editable={!busy}
                />
              ))}
            <Pressable
              style={[styles.button, busy && styles.buttonDisabled]}
              onPress={() => runAction(name, action)}
              disabled={busy}
            >
              {busy ? (
                <ActivityIndicator color="#fff" />
              ) : (
                <Text style={styles.buttonText}>{prettify(action.description ?? name)}</Text>
              )}
            </Pressable>
          </View>
        ))}

        {withoutInputs.length > 0 && (
          <View style={styles.secondaryRow}>
            {withoutInputs.map(([name, action]) => (
              <Pressable key={name} onPress={() => runAction(name, action)} disabled={busy}>
                <Text style={styles.link}>{prettify(action.description ?? name)}</Text>
              </Pressable>
            ))}
          </View>
        )}
      </ScrollView>
    </KeyboardAvoidingView>
  );
}

function prettify(s: string): string {
  const withSpaces = s.replace(/[_-]/g, ' ');
  return withSpaces.charAt(0).toUpperCase() + withSpaces.slice(1);
}

const styles = StyleSheet.create({
  flex: { flex: 1, backgroundColor: '#faf8f3' },
  container: { flexGrow: 1, justifyContent: 'center', padding: 24, gap: 16 },
  centered: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 12, padding: 24 },
  title: { fontSize: 32, fontWeight: '700', color: '#2b2a26', textAlign: 'center' },
  subtitle: { fontSize: 15, color: '#6b6456', textAlign: 'center', marginBottom: 8 },
  card: { gap: 10 },
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
  },
  buttonDisabled: { opacity: 0.6 },
  buttonText: { color: '#fff', fontWeight: '600', fontSize: 16 },
  secondaryButton: {
    borderWidth: 1,
    borderColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 10,
    paddingHorizontal: 20,
  },
  secondaryButtonText: { color: '#3b6e5e', fontWeight: '600' },
  secondaryRow: { alignItems: 'center', gap: 8, marginTop: 4 },
  link: { color: '#3b6e5e', fontWeight: '500' },
  error: { color: '#b3432b', textAlign: 'center' },
});
