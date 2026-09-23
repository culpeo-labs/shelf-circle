import { ImageManipulator, SaveFormat } from 'expo-image-manipulator';
import * as ImagePicker from 'expo-image-picker';

import { createAvatarUpload } from '../api/endpoints';
import { ApiError } from '../api/client';

const AVATAR_SIZE_PX = 512;

export interface PickedAvatar {
  /** Local file URI of the resized JPEG — for an immediate preview. */
  localUri: string;
  /** Public URL the image is now stored at; pass to `PATCH /me { avatar_url }`. */
  avatarUrl: string;
}

/**
 * Let the user pick + square-crop a photo, shrink it to a small JPEG, and
 * upload it straight to Blob Storage via a short-lived URL from the API (image
 * bytes never pass through the API). Resolves `null` if they cancel. Nothing
 * about the profile changes until the caller saves `avatarUrl`.
 */
export async function pickAndUploadAvatar(): Promise<PickedAvatar | null> {
  const picked = await ImagePicker.launchImageLibraryAsync({
    mediaTypes: ['images'],
    allowsEditing: true,
    aspect: [1, 1],
    quality: 1,
  });
  if (picked.canceled) return null;

  const rendered = await ImageManipulator.manipulate(picked.assets[0].uri)
    .resize({ width: AVATAR_SIZE_PX })
    .renderAsync();
  const resized = await rendered.saveAsync({ format: SaveFormat.JPEG, compress: 0.8 });

  const ticket = await createAvatarUpload();
  const body = await (await fetch(resized.uri)).blob();

  let response: Response;
  try {
    response = await fetch(ticket.upload_url, {
      method: 'PUT',
      headers: { 'x-ms-blob-type': 'BlockBlob', 'Content-Type': 'image/jpeg' },
      body,
    });
  } catch {
    throw new ApiError(0, 'Could not upload the photo. Check your connection and try again.');
  }
  if (!response.ok) throw new ApiError(response.status, 'Could not upload the photo.');

  return { localUri: resized.uri, avatarUrl: ticket.avatar_url };
}
