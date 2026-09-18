import { createNativeStackNavigator } from '@react-navigation/native-stack';
import React from 'react';

import { useAuth } from '../auth/AuthContext';
import { LoadingScreen } from '../components/StatusViews';
import { AuthFlowScreen } from '../screens/auth/AuthFlowScreen';
import { CreateIdentityScreen } from '../screens/onboarding/CreateIdentityScreen';
import { AddPastReadScreen } from '../screens/books/AddPastReadScreen';
import { BookDetailScreen } from '../screens/books/BookDetailScreen';
import { BookSearchScreen } from '../screens/books/BookSearchScreen';
import { FinishBookScreen } from '../screens/books/FinishBookScreen';
import { AddFriendScreen } from '../screens/friends/AddFriendScreen';
import { FriendProfileScreen } from '../screens/friends/FriendProfileScreen';
import { RecommendToFriendScreen } from '../screens/recommendations/RecommendToFriendScreen';
import { MainTabs } from './MainTabs';
import type { RootStackParamList } from './types';

const Stack = createNativeStackNavigator<RootStackParamList>();

export function RootNavigator() {
  const { status } = useAuth();

  if (status === 'loading') return <LoadingScreen />;

  return (
    <Stack.Navigator>
      {status === 'signed-out' && (
        <Stack.Screen name="AuthFlow" component={AuthFlowScreen} options={{ headerShown: false }} />
      )}

      {status === 'onboarding' && (
        <Stack.Screen name="CreateIdentity" component={CreateIdentityScreen} options={{ headerShown: false }} />
      )}

      {status === 'signed-in' && (
        <>
          <Stack.Screen name="Main" component={MainTabs} options={{ headerShown: false }} />
          <Stack.Screen name="BookDetail" component={BookDetailScreen} options={{ title: 'Book' }} />
          <Stack.Screen
            name="BookSearch"
            component={BookSearchScreen}
            options={{ title: 'Find a book', presentation: 'modal' }}
          />
          <Stack.Screen
            name="AddPastRead"
            component={AddPastReadScreen}
            options={{ title: "Add a book I've read", presentation: 'modal' }}
          />
          <Stack.Screen
            name="FinishBook"
            component={FinishBookScreen}
            options={{ title: 'Update shelf', presentation: 'modal' }}
          />
          <Stack.Screen
            name="RecommendToFriend"
            component={RecommendToFriendScreen}
            options={{ title: 'Recommend', presentation: 'modal' }}
          />
          <Stack.Screen
            name="AddFriend"
            component={AddFriendScreen}
            options={{ title: 'Add a friend', presentation: 'modal' }}
          />
          <Stack.Screen name="FriendProfile" component={FriendProfileScreen} options={{ title: 'Friend' }} />
        </>
      )}
    </Stack.Navigator>
  );
}
