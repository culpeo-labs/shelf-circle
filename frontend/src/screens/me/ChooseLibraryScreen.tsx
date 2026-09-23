import type { NativeStackScreenProps } from '@react-navigation/native-stack';
import React from 'react';
import { FlatList, Pressable, StyleSheet, Text, View } from 'react-native';

import { ApiError } from '../../api/client';
import { ErrorRetry, LoadingScreen } from '../../components/StatusViews';
import { useLibrarySystems, useMyLibrarySystem, useSetMyLibrarySystem } from '../../hooks/queries';
import type { RootStackParamList } from '../../navigation/types';

type Props = NativeStackScreenProps<RootStackParamList, 'ChooseLibrary'>;

/** Pick the library system used for "Get it at your library" links. The list
 * comes from the server, so new libraries appear without an app update. */
export function ChooseLibraryScreen({ navigation }: Props) {
  const systems = useLibrarySystems();
  const mine = useMyLibrarySystem();
  const setSystem = useSetMyLibrarySystem();
  const selectedId = mine.data?.library_system?.id ?? null;

  if (systems.isLoading || mine.isLoading) return <LoadingScreen />;
  if (systems.isError || !systems.data) {
    return <ErrorRetry message="Couldn't load libraries." onRetry={() => void systems.refetch()} />;
  }

  function choose(id: string | null) {
    if (id === selectedId) return navigation.goBack();
    setSystem.mutate(id, { onSuccess: () => navigation.goBack() });
  }

  return (
    <View style={styles.flex}>
      <Text style={styles.intro}>
        Which library do you borrow from? Book pages will link straight to the book in its catalog.
      </Text>
      {setSystem.isError && (
        <Text style={styles.error}>
          {setSystem.error instanceof ApiError ? setSystem.error.message : 'Could not save that.'}
        </Text>
      )}
      <FlatList
        data={systems.data}
        keyExtractor={(s) => s.id}
        ListFooterComponent={
          <Row
            label="None"
            selected={selectedId === null}
            disabled={setSystem.isPending}
            onPress={() => choose(null)}
          />
        }
        renderItem={({ item }) => (
          <Row
            label={item.name}
            selected={item.id === selectedId}
            disabled={setSystem.isPending}
            onPress={() => choose(item.id)}
          />
        )}
      />
    </View>
  );
}

function Row({
  label,
  selected,
  disabled,
  onPress,
}: {
  label: string;
  selected: boolean;
  disabled: boolean;
  onPress: () => void;
}) {
  return (
    <Pressable style={styles.row} onPress={onPress} disabled={disabled}>
      <Text style={styles.rowLabel}>{label}</Text>
      {selected && <Text style={styles.check}>✓</Text>}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  flex: { flex: 1, backgroundColor: '#faf8f3' },
  intro: { padding: 16, color: '#6b6456', fontSize: 14 },
  error: { color: '#b3432b', paddingHorizontal: 16, paddingBottom: 8 },
  row: {
    flexDirection: 'row',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingVertical: 16,
    paddingHorizontal: 16,
    borderTopWidth: StyleSheet.hairlineWidth,
    borderTopColor: '#d9d3c4',
  },
  rowLabel: { fontSize: 16, color: '#2b2a26' },
  check: { fontSize: 18, color: '#3b6e5e', fontWeight: '700' },
});
