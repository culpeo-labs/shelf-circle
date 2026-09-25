import React, { useState } from 'react';
import { ActivityIndicator, Pressable, StyleSheet, Text, TextInput, View } from 'react-native';

import { useDeleteReadingGoal, useReadingStats, useSetReadingGoal } from '../hooks/queries';

/**
 * "Reading this year": books finished so far, and progress toward an optional
 * yearly goal. Counts come from the server (rereads count each time; backlog
 * books logged as read before using the app don't) and follow the device's
 * time zone for the year boundary.
 */
export function ReadingYearCard() {
  const year = new Date().getFullYear();
  const stats = useReadingStats(year);
  const setGoal = useSetReadingGoal();
  const clearGoal = useDeleteReadingGoal();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');

  if (stats.isLoading) {
    return (
      <View style={styles.card}>
        <ActivityIndicator color="#3b6e5e" />
      </View>
    );
  }
  if (stats.isError || !stats.data) return null; // a stats hiccup shouldn't clutter the profile

  const { completed, goal } = stats.data;
  const progress = goal ? Math.min(1, completed / goal.target_count) : 0;
  const target = Number.parseInt(draft, 10);
  const validTarget = Number.isInteger(target) && target >= 1 && target <= 10000;

  function startEditing() {
    setDraft(goal ? String(goal.target_count) : '');
    setEditing(true);
  }

  async function save() {
    if (!validTarget) return;
    await setGoal.mutateAsync({ year, target });
    setEditing(false);
  }

  return (
    <View style={styles.card}>
      <Text style={styles.label}>Reading in {year}</Text>
      <Text style={styles.count}>
        {completed} {completed === 1 ? 'book' : 'books'}
        {goal ? <Text style={styles.of}> of {goal.target_count}</Text> : null}
      </Text>

      {goal && (
        <View style={styles.track}>
          <View style={[styles.fill, { width: `${progress * 100}%` }]} />
        </View>
      )}

      {editing ? (
        <View style={styles.editRow}>
          <TextInput
            style={styles.input}
            placeholderTextColor="#918a78"
            placeholder="Books this year"
            value={draft}
            onChangeText={(t) => setDraft(t.replace(/[^0-9]/g, ''))}
            keyboardType="number-pad"
            maxLength={5}
            autoFocus
          />
          <Pressable onPress={save} disabled={!validTarget || setGoal.isPending} hitSlop={8}>
            <Text style={[styles.action, (!validTarget || setGoal.isPending) && styles.disabled]}>
              Save
            </Text>
          </Pressable>
          <Pressable onPress={() => setEditing(false)} hitSlop={8}>
            <Text style={styles.cancel}>Cancel</Text>
          </Pressable>
        </View>
      ) : (
        <View style={styles.editRow}>
          <Pressable onPress={startEditing} hitSlop={8}>
            <Text style={styles.action}>{goal ? 'Change goal' : 'Set a yearly goal'}</Text>
          </Pressable>
          {goal && (
            <Pressable
              onPress={() => clearGoal.mutate(year)}
              disabled={clearGoal.isPending}
              hitSlop={8}
            >
              <Text style={styles.cancel}>Remove</Text>
            </Pressable>
          )}
        </View>
      )}
      {(setGoal.isError || clearGoal.isError) && (
        <Text style={styles.error}>Couldn’t save that. Try again.</Text>
      )}
    </View>
  );
}

const styles = StyleSheet.create({
  card: {
    alignSelf: 'stretch',
    marginTop: 24,
    padding: 16,
    gap: 8,
    borderRadius: 12,
    borderWidth: 1,
    borderColor: '#e5e1d8',
    backgroundColor: '#fff',
  },
  label: { fontSize: 13, fontWeight: '600', color: '#6b6456', textTransform: 'uppercase' },
  count: { fontSize: 28, fontWeight: '700', color: '#2b2a26' },
  of: { fontSize: 18, fontWeight: '500', color: '#6b6456' },
  track: { height: 8, borderRadius: 4, backgroundColor: '#e5e1d8', overflow: 'hidden' },
  fill: { height: 8, borderRadius: 4, backgroundColor: '#3b6e5e' },
  editRow: { flexDirection: 'row', alignItems: 'center', gap: 16, marginTop: 4 },
  input: {
    flex: 1,
    borderWidth: 1,
    borderColor: '#d9d3c4',
    borderRadius: 8,
    paddingHorizontal: 12,
    paddingVertical: 8,
    fontSize: 16,
    backgroundColor: '#fff',
    color: '#2b2a26',
  },
  action: { color: '#3b6e5e', fontWeight: '600', fontSize: 14 },
  cancel: { color: '#6b6456', fontSize: 14 },
  disabled: { opacity: 0.4 },
  error: { color: '#b3432b', fontSize: 12 },
});
