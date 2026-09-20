import { Image } from 'expo-image';
import React, { useState } from 'react';
import { StyleSheet, Text, View } from 'react-native';

interface Props {
  url: string | null;
  title: string;
  width?: number;
  height?: number;
}

export function BookCover({ url, title, width = 48, height = 72 }: Props) {
  const [failed, setFailed] = useState(false);
  const showPlaceholder = !url || failed;

  if (showPlaceholder) {
    return (
      <View style={[styles.placeholder, { width, height }]}>
        <Text style={styles.placeholderText} numberOfLines={4}>
          {title}
        </Text>
      </View>
    );
  }

  return (
    <Image
      source={{ uri: url }}
      style={[styles.image, { width, height }]}
      contentFit="cover"
      onError={() => setFailed(true)}
      accessibilityLabel={`Cover of ${title}`}
    />
  );
}

const styles = StyleSheet.create({
  image: {
    borderRadius: 4,
    backgroundColor: '#e5e1d8',
  },
  placeholder: {
    borderRadius: 4,
    backgroundColor: '#e5e1d8',
    alignItems: 'center',
    justifyContent: 'center',
    padding: 4,
  },
  placeholderText: {
    fontSize: 10,
    textAlign: 'center',
    color: '#6b6456',
  },
});
