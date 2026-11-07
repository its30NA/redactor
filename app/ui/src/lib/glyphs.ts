/** One small icon per kind of finding, so the rail reads at a glance. */
import type { LucideIcon } from '@lucide/svelte';
import Asterisk from '@lucide/svelte/icons/asterisk';
import AtSign from '@lucide/svelte/icons/at-sign';
import Binary from '@lucide/svelte/icons/binary';
import Bookmark from '@lucide/svelte/icons/bookmark';
import CreditCard from '@lucide/svelte/icons/credit-card';
import KeyRound from '@lucide/svelte/icons/key-round';
import Landmark from '@lucide/svelte/icons/landmark';
import Network from '@lucide/svelte/icons/network';
import PenLine from '@lucide/svelte/icons/pen-line';
import Phone from '@lucide/svelte/icons/phone';
import Shuffle from '@lucide/svelte/icons/shuffle';
import Tag from '@lucide/svelte/icons/tag';
import TextCursorInput from '@lucide/svelte/icons/text-cursor-input';
import Ticket from '@lucide/svelte/icons/ticket';
import type { Source } from './types';

export function glyphFor(kind: string, source: Source): LucideIcon {
  if (source === 'manual') return TextCursorInput;
  if (source === 'watchlist') return Bookmark;
  if (source === 'custom') return Tag;
  if (source === 'decoded') return Binary;
  const k = kind.toLowerCase();
  if (k.includes('password') || k.includes('passphrase') || k.includes('pin')) return Asterisk;
  if (k.includes('jwt') || k.includes('session') || k.includes('cookie')) return Ticket;
  if (k.includes('signing') || k.includes('signature') || k.includes('webhook')) return PenLine;
  if (k.includes('email') || k.includes('mail')) return AtSign;
  if (k.includes('phone')) return Phone;
  if (k.includes('card')) return CreditCard;
  if (k.includes('iban') || k.includes('bank')) return Landmark;
  if (k.includes('ip') || k.includes('host')) return Network;
  if (k.includes('random') || k.includes('entropy')) return Shuffle;
  return KeyRound;
}
