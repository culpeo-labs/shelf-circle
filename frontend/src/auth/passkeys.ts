import type * as Passkeys from 'react-native-passkeys';

export type PasskeyModule = typeof Passkeys;

let cached: PasskeyModule | null | undefined;

/**
 * The passkey module, or null where it isn't available. It's a native module,
 * so it's loaded lazily: in Expo Go (no custom native code) the require throws,
 * and the app just doesn't offer passkey sign-in.
 */
export function passkeys(): PasskeyModule | null {
  if (cached === undefined) {
    try {
      const mod = require('react-native-passkeys') as PasskeyModule;
      cached = mod.isSupported() ? mod : null;
    } catch {
      cached = null;
    }
  }
  return cached;
}
