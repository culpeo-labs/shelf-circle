import React from 'react';
import { Pressable, StyleSheet, Text, View } from 'react-native';

interface Props {
  value: number | null;
  onChange?: (value: number | null) => void;
  size?: number;
}

/** 1-5 star picker. Tapping the currently-selected star clears the rating. */
export function StarRating({ value, onChange, size = 28 }: Props) {
  const editable = !!onChange;

  return (
    <View style={styles.row}>
      {[1, 2, 3, 4, 5].map((star) => {
        const filled = value !== null && star <= value;
        const star_ = (
          <Text
            key={star}
            style={{ fontSize: size, color: filled ? '#d9a441' : '#d9d3c4' }}
            accessibilityLabel={`${star} star${star === 1 ? '' : 's'}`}
          >
            {filled ? '★' : '☆'}
          </Text>
        );
        if (!editable) return star_;
        return (
          <Pressable
            key={star}
            onPress={() => onChange!(value === star ? null : star)}
            hitSlop={6}
            accessibilityRole="button"
          >
            {star_}
          </Pressable>
        );
      })}
    </View>
  );
}

const styles = StyleSheet.create({
  row: { flexDirection: 'row', gap: 2 },
});
