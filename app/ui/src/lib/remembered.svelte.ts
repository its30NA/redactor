/**
 * What the Remembered sheet does with an Always redact entry that looks like a password.
 * "Keep for this session" sends the value to the shell's memory and never to the settings;
 * "Add anyway" stores it in the list like any other word.
 */
import { addAnyway, TYPED_SECRET_LABEL } from './always-redact';
import { session } from './session.svelte';
import { settings } from './settings.svelte';

/** True when the shell took the value into memory. */
export function keepForSession(value: string): Promise<boolean> {
  return session.add(value.trim(), TYPED_SECRET_LABEL);
}

/** Null once the word is on the list, otherwise the reason it was refused. */
export async function addToList(value: string): Promise<string | null> {
  const res = addAnyway(settings.value.alwaysRedact, value);
  if ('error' in res) return res.error;
  await settings.update({ alwaysRedact: res.list });
  return null;
}
