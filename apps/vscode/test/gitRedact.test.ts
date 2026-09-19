// TypeScript counterpart of `crates/gitsail-domain/src/redact.rs`'s own
// tests, including its sentinel case.
//
// ADR-025 duplicated this logic into the extension (see
// `src/git/redact.ts` for why it could not simply be skipped). Keeping the
// Rust suite's assertions verbatim is what makes the duplication
// auditable: if one side ever stops redacting a shape the other still
// does, one of these two suites goes red.

import { describe, expect, it } from "vitest";

import { redactCredentialUrl, redactSecrets } from "../src/git/redact";

describe("redactCredentialUrl", () => {
  it("masks credentials embedded in a URL", () => {
    const redacted = redactCredentialUrl("https://user:secret-token@github.com/org/repo.git");
    expect(redacted).toBe("https://***@github.com/org/repo.git");
    expect(redacted).not.toContain("secret-token");
  });

  it("leaves non-credential text untouched", () => {
    expect(redactCredentialUrl("--porcelain=v2")).toBe("--porcelain=v2");
    expect(redactCredentialUrl("git@github.com:org/repo.git")).toBe("git@github.com:org/repo.git");
    expect(redactCredentialUrl("https://github.com/org/repo.git")).toBe(
      "https://github.com/org/repo.git",
    );
  });

  it("does not treat an '@' in the path as a credentials separator", () => {
    const url = "https://github.com/org/repo/blob/main/a@b.txt";
    expect(redactCredentialUrl(url)).toBe(url);
  });
});

describe("redactSecrets", () => {
  const SENTINEL = "sentinel-fake-token-9f3c7a";

  it("never lets a sentinel secret survive, in any shape GitSail is likely to see it", () => {
    const urlCase = `fatal: could not read Username for 'https://user:${SENTINEL}@github.com/org/repo.git'`;
    expect(redactSecrets(urlCase)).not.toContain(SENTINEL);

    const kvCase = `request failed token=${SENTINEL} retrying`;
    expect(redactSecrets(kvCase)).not.toContain(SENTINEL);

    const headerCase = `Authorization: Bearer ${SENTINEL}`;
    expect(redactSecrets(headerCase)).not.toContain(SENTINEL);

    const passwordCase = `password=${SENTINEL}`;
    expect(redactSecrets(passwordCase)).not.toContain(SENTINEL);
  });

  it("preserves the surrounding diagnostic text so the message stays useful", () => {
    const redacted = redactSecrets('exit_code=128 args=["fetch"] stderr=fatal: authentication failed');
    expect(redacted).toContain("exit_code=128");
    expect(redacted).toContain("authentication failed");
  });

  it("redacts line by line across multiline text", () => {
    const redacted = redactSecrets("line one\nAuthorization: Bearer abc123\nline three");
    expect(redacted).toContain("line one");
    expect(redacted).toContain("line three");
    expect(redacted).not.toContain("abc123");
  });

  it("keeps the header name and scheme visible, redacting only the credential", () => {
    expect(redactSecrets("Authorization: Bearer abc123")).toBe("Authorization: Bearer ***");
  });
});
