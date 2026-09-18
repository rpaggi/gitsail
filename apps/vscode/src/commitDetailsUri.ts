// Encoding/decoding `gitsail-commit:` URIs (T-206 criterion 2: "abre
// detalhes completos... chama o CLI de novo... não reaproveita string
// parseada da decoração"). A commit-details document is opened the same
// read-only, never-written-to-disk way a historical file version is
// (`historyUri.ts`) — this is a sibling scheme rather than reusing
// `gitsail-history:`, since a commit's details are not "a file's content
// at a revision" at all.

export const COMMIT_DETAILS_URI_SCHEME = "gitsail-commit";

export interface CommitDetailsUriParams {
  repoRoot: string;
  hash: string;
}

export interface UriLike {
  scheme: string;
  path: string;
  query: string;
}

export function buildCommitDetailsUriString(params: CommitDetailsUriParams): string {
  return `${COMMIT_DETAILS_URI_SCHEME}:/${params.hash}.gitsail-commit?${new URLSearchParams({
    repo: params.repoRoot,
    hash: params.hash,
  }).toString()}`;
}

export function parseCommitDetailsUri(uri: UriLike): CommitDetailsUriParams | undefined {
  if (uri.scheme !== COMMIT_DETAILS_URI_SCHEME) {
    return undefined;
  }
  const query = new URLSearchParams(uri.query);
  const repoRoot = query.get("repo");
  const hash = query.get("hash");
  if (!repoRoot || !hash) {
    return undefined;
  }
  return { repoRoot, hash };
}
