// Sanitizing repository-authored text (commit subjects/bodies) before it
// ever reaches a `vscode.MarkdownString`-rendered hover (T-206 criterion 3).
//
// This is a real security boundary, not a style preference: a commit
// message is untrusted, repository-authored content — anyone who can push a
// commit (or craft one locally and get someone else to open it) controls
// this text. Markdown hovers can render active links, including a
// `command:` URI scheme that *executes a VS Code command* when clicked if
// the `MarkdownString` is `isTrusted`. A message like
// `[Click for details](command:workbench.action.terminal.new)` must never
// become a clickable, executable link just because it appeared in a commit
// message.
//
// Two independent layers, deliberately redundant (defense in depth, per the
// task's explicit security requirement):
//   1. `escapeMarkdownText` neutralizes every Markdown control character in
//      untrusted text, so it can only ever render as literal text, never as
//      structure (links, headings, emphasis, HTML).
//   2. The caller (`extension.ts`) additionally always constructs the real
//      `vscode.MarkdownString` with `isTrusted: false` explicitly set, so
//      even a `command:`/`file:` link that somehow survived escaping (e.g.
//      a future bug in this module) still cannot execute anything — VS Code
//      itself refuses to resolve command links on an untrusted
//      `MarkdownString`.
//
// GitSail's own UI text (labels this extension writes itself, e.g. "GitSail
// blame details") is never passed through this escaper — only repository
// content is untrusted.

/** Characters Markdown gives special meaning to, per CommonMark's ASCII
 * punctuation set (a superset is safe to escape; under-escaping is the
 * dangerous direction here, not over-escaping plain text). */
const MARKDOWN_SPECIAL_CHARS = /[\\`*_{}[\]()#+\-.!|<>~^]/g;

/** Escapes every Markdown control character in `text` with a backslash, so
 * it can only ever render as literal text. Applied per-character (not by
 * pattern-matching specific "dangerous" constructs like `command:` links)
 * because pattern-matching known-bad constructs is inherently incomplete —
 * escaping every control character is the only approach that does not
 * depend on anticipating every way Markdown could be abused. */
export function escapeMarkdownText(text: string): string {
  return text.replace(MARKDOWN_SPECIAL_CHARS, (match) => `\\${match}`);
}

/** A hover body built from lines this extension already trusts (its own
 * labels) or has explicitly sanitized (repository content) — the type
 * itself does not enforce that distinction (a plain string can't), so
 * every call site must escape untrusted content *before* it reaches this
 * function; see `HoverContentBuilder` below for the safer, structured way
 * to do that. */
export interface SafeHoverContent {
  /** Markdown source, safe to render with `isTrusted: false` — or, when
   * `enabledCommands` is non-empty, `isTrusted: { enabledCommands }` (see
   * `addCommandLink`'s doc comment for why a scoped allow-list, not a blanket
   * `true`, is the right trust level here). */
  readonly markdown: string;
  /** Command ids this content's own `addCommandLink` calls reference —
   * exactly the allow-list a caller must pass as
   * `MarkdownString.isTrusted.enabledCommands`. Deriving this from what was
   * actually built (rather than a caller hand-maintaining a matching list
   * separately) means the allow-list can never drift out of sync with the
   * links this content actually contains. */
  readonly enabledCommands: readonly string[];
}

/**
 * Builds a hover's Markdown source from a mix of trusted (GitSail-authored)
 * and untrusted (repository-authored) line fragments, escaping only the
 * untrusted ones. Using this builder instead of manual string
 * concatenation is what keeps "did I remember to escape this one" from
 * being a per-call-site judgment call.
 */
export class HoverContentBuilder {
  private readonly lines: string[] = [];
  private readonly commandIds = new Set<string>();

  /** Adds a line of GitSail's own UI text verbatim (e.g. a label like
   * `"Uncommitted change"`) — never repository content. */
  addTrustedLine(line: string): this {
    this.lines.push(line);
    return this;
  }

  /**
   * Adds a Markdown link that invokes one of this extension's own commands
   * (T-206 criterion 2: "uma ação... abre detalhes completos"), reconciled
   * with criterion 3's security requirement: `isTrusted: true` (blanket
   * trust) would let *any* `command:` link execute — including one hidden
   * in escaped-but-not-perfectly-escaped repository content, now or after a
   * future bug in `escapeMarkdownText` — while `isTrusted: false` would
   * make this link (and every other command link) inert, including this
   * legitimate one. VS Code's actual answer to that tension is a *scoped*
   * trust: `isTrusted: { enabledCommands: [...] }`, which only ever
   * executes the specific command ids this content itself declares.
   * `label` must be this extension's own fixed text, never repository
   * content — it is written directly into Markdown link syntax and would
   * otherwise be exactly the injection this module exists to prevent.
   * `args` is JSON-encoded into the command URI, matching VS Code's own
   * `command:` URI argument convention.
   */
  addCommandLink(label: string, commandId: string, args?: unknown): this {
    this.commandIds.add(commandId);
    const encodedArgs = args === undefined ? "" : `?${encodeURIComponent(JSON.stringify(args))}`;
    this.lines.push(`[${label}](command:${commandId}${encodedArgs})`);
    return this;
  }

  /** Adds a line of repository-authored text (a commit subject/body line,
   * an author name/email, ...), escaping it first. Still safe to call with
   * GitSail's own text — escaping plain text is a no-op for anything that
   * was not already Markdown syntax. */
  addUntrustedLine(line: string): this {
    this.lines.push(escapeMarkdownText(line));
    return this;
  }

  /** A blank line, rendered as a Markdown paragraph break. */
  addBlankLine(): this {
    this.lines.push("");
    return this;
  }

  build(): SafeHoverContent {
    // Two trailing spaces + newline is Markdown's explicit line-break
    // syntax; without it, adjacent lines collapse onto one line.
    return { markdown: this.lines.join("  \n"), enabledCommands: [...this.commandIds] };
  }
}
