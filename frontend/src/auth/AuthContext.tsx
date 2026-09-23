import * as SecureStore from 'expo-secure-store';
import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';

import { ApiError } from '../api/client';
import { setAuthToken as setApiAuthToken } from '../api/client';
import { getMe } from '../api/endpoints';
import type { User } from '../api/types';

const TOKEN_KEY = 'sc_auth_token';
const USER_KEY = 'sc_current_user';

export type AuthStatus = 'loading' | 'signed-out' | 'onboarding' | 'signed-in';

interface AuthContextValue {
  status: AuthStatus;
  user: User | null;
  /**
   * Called once a Hanko flow issues a session token. Checks whether a
   * shelf-circle profile already exists for it (`GET /me`) and moves to
   * onboarding or straight into the app accordingly.
   */
  completeWithToken: (token: string) => Promise<void>;
  /** Called once `POST /users` succeeds during onboarding. */
  completeOnboarding: (user: User) => Promise<void>;
  /** Replace the cached profile after an edit (e.g. `PATCH /me`). */
  updateUser: (user: User) => Promise<void>;
  signOut: () => Promise<void>;
}

const AuthContext = createContext<AuthContextValue | null>(null);

export function AuthProvider({ children }: { children: React.ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>('loading');
  const [user, setUser] = useState<User | null>(null);

  const clearStoredSession = useCallback(async () => {
    await SecureStore.deleteItemAsync(TOKEN_KEY);
    await SecureStore.deleteItemAsync(USER_KEY);
    setApiAuthToken(null);
  }, []);

  const restore = useCallback(async () => {
    const token = await SecureStore.getItemAsync(TOKEN_KEY);
    if (!token) {
      setStatus('signed-out');
      return;
    }
    setApiAuthToken(token);
    try {
      const me = await getMe();
      setUser(me);
      setStatus('signed-in');
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) {
        setStatus('onboarding');
        return;
      }
      // Hanko session tokens are short-lived and this app doesn't implement
      // silent refresh yet — an expired token (401) just sends the user back
      // through login. Any other failure does too, conservatively.
      await clearStoredSession();
      setStatus('signed-out');
    }
  }, [clearStoredSession]);

  useEffect(() => {
    void restore();
  }, [restore]);

  const completeWithToken = useCallback(async (token: string) => {
    await SecureStore.setItemAsync(TOKEN_KEY, token);
    setApiAuthToken(token);
    try {
      const me = await getMe();
      await SecureStore.setItemAsync(USER_KEY, JSON.stringify(me));
      setUser(me);
      setStatus('signed-in');
    } catch (e) {
      if (e instanceof ApiError && e.status === 404) {
        setStatus('onboarding');
        return;
      }
      throw e;
    }
  }, []);

  const completeOnboarding = useCallback(async (newUser: User) => {
    await SecureStore.setItemAsync(USER_KEY, JSON.stringify(newUser));
    setUser(newUser);
    setStatus('signed-in');
  }, []);

  const updateUser = useCallback(async (updated: User) => {
    await SecureStore.setItemAsync(USER_KEY, JSON.stringify(updated));
    setUser(updated);
  }, []);

  const signOut = useCallback(async () => {
    await clearStoredSession();
    setUser(null);
    setStatus('signed-out');
  }, [clearStoredSession]);

  const value = useMemo(
    () => ({ status, user, completeWithToken, completeOnboarding, updateUser, signOut }),
    [status, user, completeWithToken, completeOnboarding, updateUser, signOut],
  );

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}
