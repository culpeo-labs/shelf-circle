import { Image } from 'expo-image';
import React, { useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';

interface Props {
  url: string | null;
  name: string;
  size?: number;
}

export function Avatar({ url, name, size = 40 }: Props) {
  const [failed, setFailed] = useState(false);
  const initials = name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0]?.toUpperCase())
    .join('');

  if (!url || failed) {
    return (
      <View style={[styles.fallback, { width: size, height: size, borderRadius: size / 2 }]}>
        <Text style={[styles.initials, { fontSize: size * 0.4 }]}>{initials || '?'}</Text>
      </View>
    );
  }

  return (
    <Image
      source={{ uri: url }}
      style={{ width: size, height: size, borderRadius: size / 2, backgroundColor: '#e5e1d8' }}
      onError={() => setFailed(true)}
      accessibilityLabel={`${name}'s avatar`}
    />
  );
}

const styles = StyleSheet.create({
  fallback: {
    backgroundColor: '#3b6e5e',
    alignItems: 'center',
    justifyContent: 'center',
  },
  initials: {
    color: '#fff',
    fontWeight: '600',
  },
});
