import { createBottomTabNavigator } from '@react-navigation/bottom-tabs';
import React from 'react';
import { Text } from 'react-native';

import { useRecommendationsBadge } from '../hooks/useRecommendationsBadge';
import { FriendsScreen } from '../screens/friends/FriendsScreen';
import { MeScreen } from '../screens/me/MeScreen';
import { MyBooksScreen } from '../screens/books/MyBooksScreen';
import { RecommendationsScreen } from '../screens/recommendations/RecommendationsScreen';
import { TimelineScreen } from '../screens/timeline/TimelineScreen';
import type { MainTabParamList } from './types';

const Tab = createBottomTabNavigator<MainTabParamList>();

const ICONS: Record<keyof MainTabParamList, string> = {
  Timeline: '🏠',
  MyBooks: '📚',
  Recommendations: '💌',
  Friends: '👥',
  Me: '🙂',
};

export function MainTabs() {
  const { unseenCount } = useRecommendationsBadge();

  return (
    <Tab.Navigator
      screenOptions={({ route }) => ({
        tabBarIcon: () => <Text style={{ fontSize: 20 }}>{ICONS[route.name as keyof MainTabParamList]}</Text>,
        tabBarActiveTintColor: '#3b6e5e',
      })}
    >
      <Tab.Screen name="Timeline" component={TimelineScreen} options={{ title: 'Timeline' }} />
      <Tab.Screen name="MyBooks" component={MyBooksScreen} options={{ title: 'My Books' }} />
      <Tab.Screen
        name="Recommendations"
        component={RecommendationsScreen}
        options={{ title: 'Recs', tabBarBadge: unseenCount > 0 ? unseenCount : undefined }}
      />
      <Tab.Screen name="Friends" component={FriendsScreen} options={{ title: 'Friends' }} />
      <Tab.Screen name="Me" component={MeScreen} options={{ title: 'Me' }} />
    </Tab.Navigator>
  );
}
