import React, { useCallback, useEffect, useRef, useState } from 'react';
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

import {
  startFlow,
  submitFlowAction,
  type FlowAction,
  type FlowResult,
  type FlowState,
} from '../../auth/hankoFlowClient';
import { useAuth } from '../../auth/AuthContext';
import { passkeys, type PasskeyModule } from '../../auth/passkeys';

/** Login-start action: asks Hanko for passkey request options. */
const PASSKEY_START = 'webauthn_generate_request_options';
/** Passkey-wait action: submits the signed credential back to Hanko. */
const PASSKEY_VERIFY = 'webauthn_verify_assertion_response';

type PasskeyRequestOptions = NonNullable<Parameters<PasskeyModule['get']>[0]>;

/**
 * Pulls the WebAuthn request options out of Hanko's start-passkey payload. The
 * payload may wrap them as `{ publicKey: {...} }` (the browser's
 * CredentialRequestOptions shape) or hand them over bare.
 */
function passkeyRequestOptions(result: FlowResult): PasskeyRequestOptions {
  const payload = result.payload as { request_options?: unknown } | undefined;
  const raw = payload?.request_options as
    | { publicKey?: PasskeyRequestOptions }
    | PasskeyRequestOptions
    | undefined;
  const options = raw && 'publicKey' in raw ? raw.publicKey : (raw as PasskeyRequestOptions);
  if (!options?.challenge) throw new Error('Could not start passkey sign-in.');
  return options;
}

/**
 * Which sign-in method the user asked for once the email is in. Password is
 * the default; "Email me a code instead" switches to the passcode.
 */
type Method = 'password' | 'passcode';

/**
 * The method chooser is a state with no inputs of its own that only routes to
 * a method. These are the two routing actions Hanko offers from it. Named here
 * (rather than picked generically) because the email-first screen depends on
 * them.
 */
const METHOD_ACTIONS: Record<Method, string> = {
  password: 'continue_to_password_login',
  passcode: 'continue_to_passcode_confirmation',
};

/** Action on the password step that switches to the email-code route. */
const CHOOSER_ACTION = 'continue_to_login_method_chooser';

/**
 * Returns the routing action to submit if `state` is the method chooser, or
 * undefined if it's a step that should be shown to the user.
 */
function chooserAction(state: FlowState, preferred: Method): string | undefined {
  const hasVisibleInputs = Object.values(state.actions).some((a) =>
    Object.values(a.inputs).some((i) => !i.hidden),
  );
  if (hasVisibleInputs) return undefined;
  const order: Method[] =
    preferred === 'password' ? ['password', 'passcode'] : ['passcode', 'password'];
  for (const method of order) {
    const name = METHOD_ACTIONS[method];
    if (state.actions[name]) return name;
  }
  return undefined;
}

/**
 * Drives Hanko's `/login` or `/registration` flow (toggled by the user) and
 * renders whatever that flow currently asks for — an email field, then a
 * password field (with an email-code fallback), or a passcode field — without
 * hardcoding most step names. See `hankoFlowClient.ts` for why.
 */
export function AuthFlowScreen() {
  const { completeWithToken } = useAuth();
  const [mode, setMode] = useState<'login' | 'registration'>('login');
  const [state, setState] = useState<FlowResult | null>(null);
  const [values, setValues] = useState<Record<string, string>>({});
  const [revealedFields, setRevealedFields] = useState<Record<string, boolean>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Read by the chooser auto-advance below; set by whichever action the user
  // just took, so going Back from the code screen lands on the password step
  // instead of bouncing straight back to the code.
  const methodRef = useRef<Method>('password');

  const applyResult = useCallback(
    // Named function expression so the recursive call below refers to this
    // function's own name binding rather than the outer `const` (which is
    // still being initialized while the callback body is being defined).
    async function applyResult(result: FlowResult): Promise<void> {
      if (result.authToken) {
        await completeWithToken(result.authToken);
        return;
      }

      const routeAction = chooserAction(result, methodRef.current);
      if (routeAction) {
        await applyResult(await submitFlowAction(result, routeAction, {}));
        return;
      }

      // An action with inputs that are all hidden (e.g. Hanko's
      // `register_client_capabilities` preflight step) is a machine-only step —
      // its values are meant to be computed and sent automatically, not
      // presented as something to tap. React Native has no WebAuthn, so there's
      // nothing meaningful to compute; submitting it empty (verified against a
      // live Hanko project) is enough to advance the flow.
      const autoAction = Object.entries(result.actions).find(
        ([, action]) =>
          Object.values(action.inputs).length > 0 &&
          Object.values(action.inputs).every((i) => i.hidden),
      );
      if (autoAction) {
        const [name] = autoAction;
        await applyResult(await submitFlowAction(result, name, {}));
        return;
      }

      setState(result);
      setValues({});
      setRevealedFields({});
      setError(result.error?.message ?? null);
    },
    [completeWithToken],
  );

  const restart = useCallback(async () => {
    setState(null);
    setBusy(true);
    setError(null);
    methodRef.current = 'password';
    try {
      await applyResult(await startFlow(mode === 'login' ? '/login' : '/registration'));
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Could not start sign-in.');
    } finally {
      setBusy(false);
    }
  }, [mode, applyResult]);

  useEffect(() => {
    void restart();
  }, [restart]);

  /**
   * Passkey sign-in: Hanko issues the challenge, the device signs it (Face ID /
   * fingerprint / PIN), and the signed credential goes back to Hanko. A cancelled
   * prompt leaves the user on the email step.
   */
  async function runPasskey(passkey: PasskeyModule) {
    if (!state) return;
    setBusy(true);
    setError(null);
    methodRef.current = 'password';
    try {
      const started = await submitFlowAction(state, PASSKEY_START, {});
      if (!started.payload) {
        await applyResult(started);
        return;
      }
      const credential = await passkey.get(passkeyRequestOptions(started));
      if (!credential) return;
      await applyResult(
        await submitFlowAction(started, PASSKEY_VERIFY, { assertion_response: credential }),
      );
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Passkey sign-in failed. Try again or use email.');
    } finally {
      setBusy(false);
    }
  }

  async function runAction(actionName: string, action: FlowAction, method: Method = 'password') {
    if (!state) return;
    setBusy(true);
    setError(null);
    methodRef.current = method;
    try {
      const inputData: Record<string, unknown> = {};
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

  // Hanko's Flow API uses the literal action name "back" consistently across
  // every flow for step navigation — it's a protocol convention, not just
  // another choice, so it gets its own back-style affordance instead of
  // sitting in the list of real choices.
  const backEntry = state.actions.back;
  const chooserEntry = state.actions[CHOOSER_ACTION];
  const passkeyEntry = state.actions[PASSKEY_START];
  const passkey = passkeyEntry ? passkeys() : null;
  // The passkey actions are driven by `runPasskey`, not rendered as form steps.
  const actionEntries = Object.entries(state.actions).filter(
    ([name]) =>
      name !== 'back' &&
      name !== CHOOSER_ACTION &&
      name !== PASSKEY_START &&
      name !== PASSKEY_VERIFY,
  );
  const withInputs = actionEntries.filter(([, a]) =>
    Object.values(a.inputs).some((i) => !i.hidden),
  );
  const withoutInputs = actionEntries.filter(
    ([, a]) => !Object.values(a.inputs).some((i) => !i.hidden),
  );

  return (
    <KeyboardAvoidingView
      style={styles.flex}
      behavior={Platform.OS === 'ios' ? 'padding' : undefined}
    >
      {backEntry && (
        <View style={styles.topBar}>
          <Pressable
            style={styles.backButton}
            onPress={() => runAction('back', backEntry)}
            disabled={busy}
            hitSlop={8}
          >
            <Text style={styles.backButtonText}>‹ Back</Text>
          </Pressable>
        </View>
      )}

      <ScrollView contentContainerStyle={styles.container} keyboardShouldPersistTaps="handled">
        <Text style={styles.title}>Shelf Circle</Text>
        <Text style={styles.subtitle}>
          {mode === 'login'
            ? 'Sign in with your email to continue.'
            : 'Create an account to get started.'}
        </Text>

        {error && <Text style={styles.error}>{error}</Text>}

        {passkey && (
          <Pressable
            style={[styles.button, busy && styles.buttonDisabled]}
            onPress={() => runPasskey(passkey)}
            disabled={busy}
          >
            <Text style={styles.buttonText}>Sign in with a passkey</Text>
          </Pressable>
        )}

        {passkey && withInputs.length > 0 && <Text style={styles.orDivider}>or</Text>}

        {withInputs.map(([name, action]) => (
          <View key={name} style={styles.card}>
            {Object.values(action.inputs)
              .filter((input) => !input.hidden)
              .map((input) => {
                const isPassword = input.type === 'password';
                const revealed = revealedFields[input.name] ?? false;
                return (
                  <View key={input.name} style={styles.inputWrapper}>
                    <TextInput
                      placeholderTextColor="#918a78"
                      style={[styles.input, isPassword && styles.inputWithToggle]}
                      placeholder={prettify(input.name)}
                      value={values[input.name] ?? ''}
                      onChangeText={(text) => setValues((v) => ({ ...v, [input.name]: text }))}
                      autoCapitalize="none"
                      autoCorrect={false}
                      keyboardType={input.name.includes('email') ? 'email-address' : 'default'}
                      secureTextEntry={isPassword && !revealed}
                      editable={!busy}
                    />
                    {isPassword && (
                      <Pressable
                        style={styles.revealToggle}
                        onPress={() =>
                          setRevealedFields((r) => ({ ...r, [input.name]: !revealed }))
                        }
                        hitSlop={8}
                      >
                        <Text style={styles.revealToggleText}>{revealed ? 'Hide' : 'Show'}</Text>
                      </Pressable>
                    )}
                  </View>
                );
              })}
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
            {name === 'password_login' && chooserEntry && (
              <Pressable
                onPress={() => runAction(CHOOSER_ACTION, chooserEntry, 'passcode')}
                disabled={busy}
                style={styles.centeredLink}
              >
                <Text style={styles.link}>Email me a code instead</Text>
              </Pressable>
            )}
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

        <Pressable
          onPress={() => setMode((m) => (m === 'login' ? 'registration' : 'login'))}
          disabled={busy}
          style={styles.modeSwitch}
        >
          <Text style={styles.link}>
            {mode === 'login' ? 'New here? Create an account' : 'Already have an account? Sign in'}
          </Text>
        </Pressable>
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
  topBar: { paddingTop: 16, paddingHorizontal: 12 },
  backButton: { alignSelf: 'flex-start', paddingVertical: 8, paddingHorizontal: 8 },
  backButtonText: { color: '#3b6e5e', fontWeight: '600', fontSize: 16 },
  title: { fontSize: 32, fontWeight: '700', color: '#2b2a26', textAlign: 'center' },
  subtitle: { fontSize: 15, color: '#6b6456', textAlign: 'center', marginBottom: 8 },
  card: { gap: 10 },
  inputWrapper: { justifyContent: 'center' },
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
  inputWithToggle: { paddingRight: 64 },
  revealToggle: { position: 'absolute', right: 12, paddingVertical: 8, paddingHorizontal: 4 },
  revealToggleText: { color: '#3b6e5e', fontWeight: '600', fontSize: 13 },
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
  modeSwitch: { alignItems: 'center', marginTop: 12 },
  centeredLink: { alignItems: 'center' },
  orDivider: { color: '#918a78', textAlign: 'center' },
  link: { color: '#3b6e5e', fontWeight: '500' },
  error: { color: '#b3432b', textAlign: 'center' },
});
