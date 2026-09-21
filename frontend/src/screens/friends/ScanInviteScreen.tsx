import { useNavigation } from '@react-navigation/native';
import type { NativeStackNavigationProp } from '@react-navigation/native-stack';
import { CameraView, useCameraPermissions } from 'expo-camera';
import React, { useState } from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';

import type { RootStackParamList } from '../../navigation/types';
import { parseInviteToken } from '../../utils/inviteLink';

export function ScanInviteScreen() {
  const navigation = useNavigation<NativeStackNavigationProp<RootStackParamList>>();
  const [permission, requestPermission] = useCameraPermissions();
  // Guards against onBarcodeScanned firing repeatedly for the same code
  // while navigation.replace is still in flight.
  const [scanned, setScanned] = useState(false);

  function handleScan(result: { data: string }) {
    if (scanned) return;
    const token = parseInviteToken(result.data);
    if (!token) return;
    setScanned(true);
    navigation.replace('AcceptInvite', { token });
  }

  if (!permission) {
    return <View style={styles.centered} />;
  }

  if (!permission.granted) {
    return (
      <View style={styles.centered}>
        <Text style={styles.message}>
          Shelf Circle needs camera access to scan a friend's invite QR code.
        </Text>
        <Pressable style={styles.button} onPress={requestPermission}>
          <Text style={styles.buttonText}>Allow camera access</Text>
        </Pressable>
      </View>
    );
  }

  return (
    <View style={styles.flex}>
      <CameraView
        style={styles.flex}
        facing="back"
        barcodeScannerSettings={{ barcodeTypes: ['qr'] }}
        onBarcodeScanned={handleScan}
      />
      <View style={styles.hintBar}>
        <Text style={styles.hintText}>Point the camera at a friend's invite QR code</Text>
      </View>
    </View>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1 },
  centered: { flex: 1, alignItems: 'center', justifyContent: 'center', gap: 16, padding: 24 },
  message: { fontSize: 15, color: '#6b6456', textAlign: 'center' },
  button: {
    backgroundColor: '#3b6e5e',
    borderRadius: 8,
    paddingVertical: 12,
    paddingHorizontal: 20,
  },
  buttonText: { color: '#fff', fontWeight: '600' },
  hintBar: {
    position: 'absolute',
    bottom: 32,
    left: 24,
    right: 24,
    backgroundColor: 'rgba(0,0,0,0.6)',
    borderRadius: 8,
    padding: 12,
  },
  hintText: { color: '#fff', textAlign: 'center', fontSize: 14 },
});
