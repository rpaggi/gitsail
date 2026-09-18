// Mirrors `gitsail_protocol::ErrorPayload` (crates/gitsail-protocol/src/error.rs).
// `remediation`/`operationId` are omitted on the wire when absent
// (`skip_serializing_if`), hence optional here too.

export interface ErrorPayload {
  code: string;
  message: string;
  remediation?: string;
  operationId?: string;
}

export function isErrorPayload(value: unknown): value is ErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    "code" in value &&
    "message" in value
  );
}
