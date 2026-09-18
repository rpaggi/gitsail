// Mirrors the wire shapes `gitsail-protocol` defines
// (crates/gitsail-protocol/src/{envelope,error}.rs) — the source of truth.
// This file only mirrors the *envelope* shape and validates it defensively;
// it must never grow into a Git/parsing rule the Core already owns
// (US-069 criterion 1: "reaproveitando o conceito de schemaVersion/envelope
// — não invente um formato próprio").
//
// Individual DTO shapes (RepositoryDto, BlameDto, ...) live in `dto.ts` —
// kept separate because they are added incrementally per story, while this
// envelope contract is fixed and shared by every command.

/** Mirrors `gitsail_protocol::ErrorPayload`. */
export interface ErrorPayload {
  code: string;
  message: string;
  remediation?: string;
  operationId?: string;
}

export interface OkEnvelope<T> {
  status: "ok";
  schemaVersion: number;
  requestId: string;
  data: T;
}

export interface ErrorEnvelope {
  status: "error";
  schemaVersion: number;
  requestId: string;
  error: ErrorPayload;
}

/** Mirrors `gitsail_protocol::Envelope<T>`. */
export type Envelope<T> = OkEnvelope<T> | ErrorEnvelope;

/**
 * Schema versions this client understands. Mirrors
 * `gitsail_protocol::SCHEMA_VERSION` (currently `1`). A future breaking
 * schema bump must be added here only once this client has actually been
 * updated to handle its shape — never widened speculatively ahead of that
 * work, or this guard stops meaning anything.
 */
export const SUPPORTED_SCHEMA_VERSIONS: readonly number[] = [1];

/** The envelope itself could not be parsed as a valid protocol response. */
export class EnvelopeParseError extends Error {
  constructor(
    message: string,
    readonly cause?: unknown,
  ) {
    super(message);
    this.name = "EnvelopeParseError";
  }
}

/**
 * The envelope parsed, but declares a `schemaVersion` this client does not
 * speak. Deliberately fatal to the call (US-069 criterion 2): this client
 * never guesses at an unfamiliar shape or falls back to re-parsing raw Git
 * output itself.
 */
export class UnsupportedSchemaVersionError extends Error {
  constructor(readonly schemaVersion: number) {
    super(
      `gitsail-cli returned schemaVersion ${schemaVersion}, which this version of the GitSail extension does not understand (supported: ${SUPPORTED_SCHEMA_VERSIONS.join(
        ", ",
      )}). Update the GitSail extension or the gitsail CLI so both sides speak the same protocol version.`,
    );
    this.name = "UnsupportedSchemaVersionError";
  }
}

function isErrorPayload(value: unknown): value is ErrorPayload {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as Record<string, unknown>).code === "string" &&
    typeof (value as Record<string, unknown>).message === "string"
  );
}

/**
 * Parses and validates one line of `gitsail-cli --json` output.
 *
 * Deliberately strict: a shape this function does not recognize is a parse
 * failure, never silently coerced into "close enough" (US-069 criterion 1).
 * `data`'s inner shape is *not* validated here — that is each DTO's own
 * concern — this only validates the envelope contract every response shares.
 */
export function parseEnvelope<T>(raw: string): Envelope<T> {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch (cause) {
    throw new EnvelopeParseError("gitsail-cli did not print valid JSON", cause);
  }

  if (typeof value !== "object" || value === null) {
    throw new EnvelopeParseError("gitsail-cli's JSON output was not an object");
  }
  const record = value as Record<string, unknown>;

  if (typeof record.schemaVersion !== "number") {
    throw new EnvelopeParseError(
      "gitsail-cli's JSON output is missing a numeric schemaVersion",
    );
  }
  if (typeof record.requestId !== "string") {
    throw new EnvelopeParseError("gitsail-cli's JSON output is missing a string requestId");
  }
  if (!SUPPORTED_SCHEMA_VERSIONS.includes(record.schemaVersion)) {
    throw new UnsupportedSchemaVersionError(record.schemaVersion);
  }

  if (record.status === "ok") {
    if (!("data" in record)) {
      throw new EnvelopeParseError('gitsail-cli\'s "ok" response is missing data');
    }
    return {
      status: "ok",
      schemaVersion: record.schemaVersion,
      requestId: record.requestId,
      data: record.data as T,
    };
  }
  if (record.status === "error") {
    if (!isErrorPayload(record.error)) {
      throw new EnvelopeParseError(
        'gitsail-cli\'s "error" response is missing a valid error payload',
      );
    }
    return {
      status: "error",
      schemaVersion: record.schemaVersion,
      requestId: record.requestId,
      error: record.error,
    };
  }
  throw new EnvelopeParseError(
    `gitsail-cli's JSON output has an unrecognized status: ${String(record.status)}`,
  );
}
