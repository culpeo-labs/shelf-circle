import React from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from 'react-native';

export function LoadingScreen() {
  return (
    <View style={styles.centered}>
      <ActivityIndicator size="large" color="#3b6e5e" />
    </View>
  );
}

export function ErrorRetry({ message, onRetry }: { message: string; onRetry: () => void }) {
  return (
    <View style={styles.centered}>
      <Text style={styles.message}>{message}</Text>
      <Pressable style={styles.button} onPress={onRetry}>
        <Text style={styles.buttonText}>Try again</Text>
      </Pressable>
    </View>
  );
}

export function EmptyState({
  title,
  action,
}: {
  title: string;
  action?: { label: string; onPress: () => void };
}) {
  return (
    <View style={styles.centered}>
      <Text style={styles.message}>{title}</Text>
      {action && (
        <Pressable style={styles.button} onPress={action.onPress}>
          <Text style={styles.buttonText}>{action.label}</Text>
        </Pressable>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  centered: {
    flex: 1,
    alignItems: 'center',
    justifyContent: 'center',
    padding: 24,
    gap: 12,
  },
  message: {
    fontSize: 15,
    color: '#6b6456',
    textAlign: 'center',
  },
  button: {
    backgroundColor: '#3b6e5e',
    paddingVertical: 10,
    paddingHorizontal: 20,
    borderRadius: 8,
  },
  buttonText: {
    color: '#fff',
    fontWeight: '600',
  },
});
