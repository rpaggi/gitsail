// Secret redaction for anything derived from `git`'s own output that could
// reach a user-visible message, a hover, or this extension's output channel.
//
// Direct TypeScript port of `gitsail_domain::redact` (see
// `crates/gitsail-domain/src/redact.rs`) — ADR-025 accepts that this
// extension now reimplements a read-only slice of the Core in TypeScript,
// and redaction is part of that slice: `git`'s stderr can itself carry a
// credential-bearing URL ("fatal: could not read Username for
// 'https://user:pass@host'"), so an extension that shows raw stderr would
// leak a secret the Rust core is careful never to leak.
//
// Deliberately conservative, for the same reason the Rust module documents:
// a superset of what "looks like" a secret is redacted, because
// under-redacting is the actual security bug.

/**
 * Masks `user:password@` credentials embedded in a `scheme://...` string.
 * Text that does not look like such a URL passes through unchanged.
 *
 * Mirrors `redact_credential_url`, including its one subtlety: an `@` that
 * appears only after a `/` belongs to the *path*, not to a credentials
 * segment, and must not be treated as one.
 */
export function redactCredentialUrl(text: string): string {
  const schemeEnd = text.indexOf("://");
  if (schemeEnd === -1) {
    return text;
  }
  const authorityStart = schemeEnd + 3;
  const rest = text.slice(authorityStart);
  const at = rest.indexOf("@");
  if (at === -1) {
    return text;
  }
  if (rest.slice(0, at).includes("/")) {
    return text;
  }
  return `${text.slice(0, authorityStart)}***@${rest.slice(at + 1)}`;
}

/** Case-insensitive keys treated as secret-bearing in a `key=value` or
 * `key: value` fragment. Mirrors `SECRET_KEYS` in the Rust module. */
const SECRET_KEYS: readonly string[] = [
  "token",
  "access_token",
  "accesstoken",
  "password",
  "passwd",
  "secret",
  "apikey",
  "api_key",
  "authorization",
];

function redactSecretsInWord(word: string): string {
  const urlRedacted = redactCredentialUrl(word);
  if (urlRedacted !== word) {
    return urlRedacted;
  }
  const eq = word.indexOf("=");
  if (eq !== -1) {
    const key = word.slice(0, eq);
    const value = word.slice(eq + 1);
    if (value.length > 0 && SECRET_KEYS.some((k) => k.toLowerCase() === key.toLowerCase())) {
      return `${key}=***`;
    }
  }
  return word;
}

function redactSecretsInLine(line: string): string {
  // `Authorization: Bearer <token>`: keep the header name and the scheme
  // (neither is a secret on its own), redact only the credential value.
  const colon = line.indexOf(":");
  if (colon !== -1) {
    const key = line.slice(0, colon);
    if (key.trim().toLowerCase() === "authorization") {
      const value = line.slice(colon + 1).replace(/^ +/, "");
      const space = value.indexOf(" ");
      if (space > 0) {
        return `${key}: ${value.slice(0, space)} ***`;
      }
      return `${key}: ***`;
    }
  }

  return line
    .split(" ")
    .map(redactSecretsInWord)
    .join(" ");
}

/**
 * Redacts every secret-shaped fragment found anywhere in free-form text
 * (`git`'s stderr, a diagnostic string, ...), preserving the surrounding
 * words and line structure so what remains is still useful for diagnosis.
 * Mirrors `redact_secrets`.
 */
export function redactSecrets(text: string): string {
  return text.split("\n").map(redactSecretsInLine).join("\n");
}
