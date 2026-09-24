import type { ReadingStatus } from '../api/types';

/**
 * One flat param list for every screen. Only a subset is ever mounted at once
 * (see `RootNavigator`, which switches groups by auth status) — a single list
 * keeps cross-group navigation typing simple without composite prop gymnastics.
 */
export type RootStackParamList = {
  AuthFlow: undefined;
  CreateIdentity: undefined;
  Main: undefined;
  BookDetail: { bookId: string };
  BookSearch: undefined;
  AddPastRead: undefined;
  FinishBook: {
    bookId: string;
    initialStatus?: Extract<ReadingStatus, 'finished' | 'did_not_finish'>;
    /** True when reached from AddPastRead — logging a book read before
     * using the app, which shouldn't show up in the timeline. */
    backdated?: boolean;
  };
  RecommendToFriend: { bookId: string };
  AddFriend: undefined;
  ScanInvite: undefined;
  AcceptInvite: { token: string };
  FriendProfile: { userId: string; displayName: string };
  EditProfile: undefined;
  ChooseLibrary: undefined;
};

export type MainTabParamList = {
  Timeline: undefined;
  MyBooks: undefined;
  Recommendations: undefined;
  Friends: undefined;
  Me: undefined;
};

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace ReactNavigation {
    interface RootParamList extends RootStackParamList {}
  }
}
