// Deterministic identicon-style avatar for a commit author.
//
// GitSail reads Git itself, and Git records only a name and an email —
// there is no avatar image anywhere in the data, and this app deliberately
// does not go fetch one (that would mean leaking every author's email hash
// to a third-party avatar host on every history page, for decoration).
// So the "avatar" is derived purely from what the commit already contains:
// initials on a color picked from the same graph-lane palette.
//
// Determinism matters more than prettiness here: the same author must get
// the same color in the commit list, the activity timeline and the commit
// details card, in this session and the next, or the avatar stops being a
// recognition aid and becomes noise.

import type { SignatureDto } from "../services/dto";

/** How many distinct avatar colors exist — the `--color-lane-*` tokens
 * (`theme.css`), reused here so avatars and graph lanes stay one palette
 * instead of two that almost match. */
export const AVATAR_COLOR_COUNT = 6;

/**
 * Up to two initials for an author.
 *
 * Prefers the display name's own word boundaries ("Ada Lovelace" -> "AL"),
 * falling back to the email's local part when the name is empty or
 * punctuation-only, and to "?" when there is nothing usable at all — a
 * commit with an empty author is malformed but does exist in the wild, and
 * rendering an empty circle would look like a loading bug.
 *
 * Only letters and digits are considered, so "  jane (bot) " yields "JB"
 * rather than "J(" — and a name written in a script without case, or in
 * one where a "word" is a single glyph, still produces something stable
 * because this never assumes Latin letters, only that the grapheme is
 * alphanumeric.
 */
export function authorInitials(author: SignatureDto): string {
  const fromName = initialsFromWords(author.name);
  if (fromName) {
    return fromName;
  }
  const localPart = author.email.split("@")[0] ?? "";
  const fromEmail = initialsFromWords(localPart.replace(/[._-]+/g, " "));
  return fromEmail || "?";
}

function initialsFromWords(source: string): string {
  const words = source
    .split(/\s+/)
    .map((word) => Array.from(word).filter((char) => /[\p{L}\p{N}]/u.test(char)))
    .filter((chars) => chars.length > 0);
  if (words.length === 0) {
    return "";
  }
  const first = words[0][0];
  const last = words.length > 1 ? words[words.length - 1][0] : "";
  return (first + last).toLocaleUpperCase();
}

/**
 * A stable color index in `[0, AVATAR_COLOR_COUNT)` for an author.
 *
 * Keyed on the lowercased email when there is one, because that is the
 * closest thing Git has to a stable author identity — the same person
 * routinely appears as "jane", "Jane Doe" and "jane doe" across a history,
 * and keying on the display name would give one human three colors. Falls
 * back to the name only when the email is empty.
 *
 * The hash is FNV-1a: not cryptographic (it does not need to be — nothing
 * here is a secret or a security boundary), just well-distributed and
 * cheap enough to run per row in a virtualized list.
 */
export function authorColorIndex(author: SignatureDto): number {
  const key = (author.email.trim() || author.name.trim()).toLocaleLowerCase();
  let hash = 0x811c9dc5;
  for (let i = 0; i < key.length; i += 1) {
    hash ^= key.charCodeAt(i);
    // `Math.imul` keeps the multiply in 32-bit space; a plain `*` would
    // silently lose precision past 2^53 and make the hash host-dependent.
    hash = Math.imul(hash, 0x01000193);
  }
  return Math.abs(hash) % AVATAR_COLOR_COUNT;
}
