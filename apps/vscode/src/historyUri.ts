// Encoding/decoding `gitsail-history:` URIs (T-209/US-076 criterion 3): a
// historical file version is opened via
// `vscode.workspace.openTextDocument(uri)` under this custom scheme, backed
// by a `TextDocumentContentProvider` this extension registers (see
// `extension.ts`), instead of ever writing the old content to a temp file
// on disk. That keeps the working tree/on-disk file untouched (criterion 3
// DoD) and makes the opened document naturally read-only (VS Code refuses
// to save a document whose scheme has no matching save handler).
//
// This module only builds/parses the URI's *string* shape — it never
// touches the real `vscode.Uri` type, so it stays testable without the
// `vscode` module (matching this package's established convention). Note
// `vscode.Uri` already exposes `.scheme`/`.path`/`.query` as plain strings,
// so `parseHistoryUri` accepts any object with that shape, including a real
// `vscode.Uri` handed in by `extension.ts` unchanged.

export const HISTORY_URI_SCHEME = "gitsail-history";

/** Sentinel `revision` value meaning "no content — this side of the
 * comparison does not exist" (US-076 criterion 1): a root commit's diff has
 * no base commit at all (not even a resolvable empty-tree *file*, since the
 * empty tree contains no blobs), so there is nothing to `show-file` query
 * for. The content provider recognizes this sentinel and short-circuits to
 * an empty document without ever calling the CLI — this is not a revision
 * `gitsail show-file` could ever resolve to, so there is no risk of it
 * colliding with a real one. */
export const EMPTY_CONTENT_REVISION = "(root)";

export interface HistoryUriParams {
  /** Absolute repository root — needed so the content provider can call
   * `gitsail show-file --repo <repoRoot> ...` without guessing which
   * repository a bare relative path belongs to. */
  repoRoot: string;
  /** Path relative to `repoRoot`, forward-slash separated. */
  filePath: string;
  /** The exact revision this content is "as of" — always a resolved commit
   * hash by the time a URI is built (never a symbolic revision like `HEAD`)
   * so a document reopened later cannot silently resolve to different
   * content (T-209 criterion 1: never fake a different comparison than the
   * one actually made). */
  revision: string;
}

export interface UriLike {
  scheme: string;
  path: string;
  query: string;
}

/** Builds the string form of a `gitsail-history:` URI. The path component
 * deliberately mirrors the file's own relative path (leading `/` plus the
 * original path, extension included) purely so VS Code's language-mode
 * detection (by extension) still applies for syntax highlighting — it is
 * not itself parsed back out of `.path`; `repoRoot`/`filePath`/`revision`
 * are always read back from the query string, which is authoritative. */
export function buildHistoryUriString(params: HistoryUriParams): string {
  const normalizedPath = params.filePath.split("\\").join("/").replace(/^\/+/, "");
  const query = new URLSearchParams({
    repo: params.repoRoot,
    path: params.filePath,
    revision: params.revision,
  }).toString();
  return `${HISTORY_URI_SCHEME}:/${normalizedPath}?${query}`;
}

/** Parses a `gitsail-history:` URI back into its parameters. Returns
 * `undefined` for anything that is not a well-formed URI of this scheme
 * (wrong scheme, or missing a required query parameter) — a content
 * provider must treat that as "nothing to show" rather than guessing. */
export function parseHistoryUri(uri: UriLike): HistoryUriParams | undefined {
  if (uri.scheme !== HISTORY_URI_SCHEME) {
    return undefined;
  }
  const query = new URLSearchParams(uri.query);
  const repoRoot = query.get("repo");
  const filePath = query.get("path");
  const revision = query.get("revision");
  if (!repoRoot || !filePath || !revision) {
    return undefined;
  }
  return { repoRoot, filePath, revision };
}
