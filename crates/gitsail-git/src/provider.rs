//! Git CLI adapter (SAD §11, §31): the substitutable [`GitCliProvider`]
//! implements [`RepositoryReadPort`] against the locally installed `git`
//! binary via [`GitProcessRunner`] — never a shell, never
//! `std::process::Command` directly.
//!
//! All parsing here uses structured, unambiguous output formats
//! (`status --porcelain=v2 -z`, custom `%x1e`/`%x1f`-delimited `git log`
//! records, `for-each-ref --format=...`, `blame --porcelain`) and never
//! depends on localized, human-readable Git text. Every invocation pins
//! `LC_ALL=C`/`LANG=C` so output is deterministic regardless of the host's
//! locale configuration. Malformed output is always reported as a
//! [`GitSailError`] with [`ErrorCode::ParseFailure`] — this adapter never
//! panics and never silently drops malformed data.
//!
//! Every result is mapped into real domain types (never ad-hoc structs);
//! all Git text parsing lives in this crate, never in `gitsail-application`
//! or `gitsail-domain`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gitsail_application::{
    BlameRequest, CommitQuery, DiffRequest, LineHistoryRequest, Page, Precondition,
    RepositoryReadPort, RepositoryWritePort, StashApplyOutcome, StashScope, TagAnnotation,
    WorktreeBranchSpec,
};
use gitsail_domain::{
    Blame, BlameLine, BlameOrigin, Branch, BranchKind, BranchName, ChangeType, Commit, CommitHash,
    Decoration, Diff, DiffHunk, DiffLine, DiffLineOrigin, ErrorCode, FileChange,
    FileContentAtRevision, FileContentKind, FileDiff, FileStatusCode, GitSailError, GitTimestamp,
    HeadState, LineHistory, LineHistoryEntry, Remote, RemoteUrl, Repository, RepositoryId,
    RepositoryStatus, ShortHash, Signature, Stash, Tag, TagKind, Worktree, WorktreeHead,
};

use crate::runner::{CancellationToken, GitProcessRunner, ProcessOutput, ProcessRequest};

/// Field separator used within a single `git log` record (`%x1f`, ASCII
/// Unit Separator). Chosen because it can never appear in ordinary commit
/// metadata, unlike a comma or pipe.
const FIELD_SEP: char = '\u{1f}';
/// Record separator between `git log` entries (`%x1e`, ASCII Record
/// Separator).
const RECORD_SEP: char = '\u{1e}';

/// `git log --pretty=format:` string producing one `FIELD_SEP`-delimited
/// record per commit, terminated by `RECORD_SEP`, in the field order
/// [`parse_log_record`] expects: hash, short hash, parent hashes, author
/// name/email, committer name/email, author date, commit date, subject,
/// body, decorations.
const LOG_FORMAT: &str = "%H\u{1f}%h\u{1f}%P\u{1f}%an\u{1f}%ae\u{1f}%cn\u{1f}%ce\u{1f}%ad\u{1f}%cd\u{1f}%s\u{1f}%b\u{1f}%D\u{1e}";

/// Number of `FIELD_SEP`-delimited fields in [`LOG_FORMAT`].
const LOG_FIELD_COUNT: usize = 12;

/// `git for-each-ref --format:` string producing one `FIELD_SEP`-delimited
/// record per ref, one per line (a refname can never contain a newline, so
/// `\n` is a safe record separator here).
const FOR_EACH_REF_FORMAT: &str =
    "%(refname)\u{1f}%(objectname)\u{1f}%(upstream:short)\u{1f}%(upstream:track)\u{1f}%(HEAD)";

/// `git for-each-ref --format:` string for `refs/tags` (EPIC-18/T-216/
/// US-091), one `RECORD_SEP`-terminated record per tag: unlike a branch
/// name, a tag's annotation message can contain embedded newlines (and, in
/// principle, arbitrary text), so this cannot use a plain one-line-per-ref
/// format like [`FOR_EACH_REF_FORMAT`] — it follows [`LOG_FORMAT`]'s
/// `RECORD_SEP`-per-record convention instead. Fields, in order: short
/// refname, the ref's own object id, that object's type (`tag` for
/// annotated, `commit` for lightweight), the annotated tag's peeled
/// (dereferenced) target commit id (empty for a lightweight tag), tagger
/// name, tagger email, tagger date (`--date=raw` shape), and the tag's full
/// message contents (empty for a lightweight tag — `%(contents)` on a
/// lightweight tag would otherwise report the *pointed-to commit's*
/// message, which [`parse_tag_record`] deliberately ignores by branching on
/// object type first).
const TAG_FORMAT: &str = "%(refname:short)\u{1f}%(objectname)\u{1f}%(objecttype)\u{1f}%(*objectname)\u{1f}%(taggername)\u{1f}%(taggeremail)\u{1f}%(taggerdate:raw)\u{1f}%(contents)\u{1e}";

/// Number of `FIELD_SEP`-delimited fields in [`TAG_FORMAT`].
const TAG_FIELD_COUNT: usize = 8;

/// `git stash list --format:` string, one `RECORD_SEP`-terminated record per
/// stash entry (EPIC-18/T-216/US-091): a stash's reflog subject can, in
/// principle, contain arbitrary text, so this follows the same
/// `RECORD_SEP`-per-record convention as [`LOG_FORMAT`]/[`TAG_FORMAT`]
/// rather than a plain one-line-per-entry format. Deliberately excludes
/// `%gd` (`stash@{N}`): combined with `--date=raw` (needed for the date
/// field below), Git renders `%gd`'s `N` as the *date* rather than the
/// stack position, so the index is instead taken from this record's
/// position in `git stash list`'s output, which is always stack order
/// (newest, `stash@{0}`, first).
const STASH_FORMAT: &str = "%H\u{1f}%gs\u{1f}%ad\u{1e}";

/// Number of `FIELD_SEP`-delimited fields in [`STASH_FORMAT`].
const STASH_FIELD_COUNT: usize = 3;

/// Default page size for [`RepositoryReadPort::commits`] when the caller
/// does not specify one. History must never assume the full log fits in
/// memory (SAD §25), so a page is always bounded.
const DEFAULT_COMMIT_LIMIT: u32 = 50;

/// Git CLI adapter implementing [`RepositoryReadPort`] against the locally
/// installed `git` binary. Substitutable: any other implementation of
/// [`RepositoryReadPort`] can stand in for it without touching application
/// or presentation code.
pub struct GitCliProvider {
    runner: GitProcessRunner,
}

impl GitCliProvider {
    pub fn new(runner: GitProcessRunner) -> Self {
        Self { runner }
    }

    /// Environment applied to every invocation so output is
    /// locale-independent (SAD §11): this adapter must never parse
    /// localized text.
    fn locale_env() -> Vec<(String, String)> {
        vec![
            ("LC_ALL".to_string(), "C".to_string()),
            ("LANG".to_string(), "C".to_string()),
        ]
    }

    /// Runs `args` in `cwd`, always pinning the `C` locale. Most of the
    /// trait this adapter implements does not (yet) expose cancellation, so
    /// a fresh, never-cancelled token is used here (SAD §39 notes this as a
    /// future evolution point); `diff` is the first read that does expose
    /// it (US-027 criterion 3) and uses [`Self::run_cancellable`] instead.
    fn run(&self, args: Vec<String>, cwd: &Path) -> Result<ProcessOutput, GitSailError> {
        self.run_cancellable(args, cwd, &CancellationToken::new())
    }

    /// Like [`Self::run`], but forwards a caller-supplied cancellation
    /// token instead of a fresh, never-cancelled one.
    fn run_cancellable(
        &self,
        args: Vec<String>,
        cwd: &Path,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, GitSailError> {
        let request = ProcessRequest::new(args, cwd.to_path_buf()).with_env(Self::locale_env());
        self.runner
            .run(request, cancel)
            .map_err(classify_index_lock_conflict)
    }

    /// Runs `args`, treating a non-zero exit as an expected "false"
    /// outcome (e.g. `rev-parse --verify` on a missing ref, or `git log`
    /// on an unborn branch) rather than a hard error: returns `Ok(None)`.
    /// Other failure kinds (timeout, cancellation, missing executable, ...)
    /// still propagate as `Err`.
    fn try_run(
        &self,
        args: Vec<String>,
        cwd: &Path,
    ) -> Result<Option<ProcessOutput>, GitSailError> {
        match self.run(args, cwd) {
            Ok(output) => Ok(Some(output)),
            Err(err) if err.code() == ErrorCode::ProcessFailure => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn stdout_string(output: &ProcessOutput) -> Result<String, GitSailError> {
        String::from_utf8(output.stdout.clone())
            .map_err(|_| parse_err("git output was not valid UTF-8"))
    }

    /// Determines [`HeadState`] for the repository rooted at `cwd`, per SAD
    /// §8: `rev-parse --verify -q HEAD` failing means no commits exist yet
    /// (`Unborn`); otherwise `symbolic-ref` tells us whether HEAD is
    /// attached to a branch or detached.
    fn determine_head_state(&self, cwd: &Path) -> Result<HeadState, GitSailError> {
        let has_head = self
            .try_run(
                vec![
                    "rev-parse".to_string(),
                    "--verify".to_string(),
                    "-q".to_string(),
                    "HEAD".to_string(),
                ],
                cwd,
            )?
            .is_some();
        if !has_head {
            return Ok(HeadState::Unborn);
        }

        let symbolic = self.try_run(
            vec![
                "symbolic-ref".to_string(),
                "-q".to_string(),
                "--short".to_string(),
                "HEAD".to_string(),
            ],
            cwd,
        )?;
        if let Some(output) = symbolic {
            let branch_name = Self::stdout_string(&output)?.trim().to_string();
            let branch = BranchName::new(branch_name)?;
            return Ok(HeadState::Attached { branch });
        }

        let hash_output = self.run(vec!["rev-parse".to_string(), "HEAD".to_string()], cwd)?;
        let hash = Self::stdout_string(&hash_output)?.trim().to_string();
        let commit = CommitHash::new(hash)?;
        Ok(HeadState::Detached { commit })
    }
}

impl RepositoryReadPort for GitCliProvider {
    fn discover(&self, path: &Path) -> Result<Repository, GitSailError> {
        // `--is-bare-repository`/`--absolute-git-dir` succeed for both bare
        // and non-bare repositories, so they are used to first establish
        // that `path` is inside a Git repository at all.
        let identity_output = self
            .try_run(
                vec![
                    "rev-parse".to_string(),
                    "--path-format=absolute".to_string(),
                    "--is-bare-repository".to_string(),
                    "--absolute-git-dir".to_string(),
                ],
                path,
            )?
            .ok_or_else(|| {
                GitSailError::new(ErrorCode::RepositoryNotFound, "not a Git repository")
                    .with_remediation("open a path inside a Git repository")
            })?;
        let stdout = Self::stdout_string(&identity_output)?;
        let mut lines = stdout.lines();
        let is_bare = match lines.next() {
            Some("true") => true,
            Some("false") => false,
            _ => {
                return Err(parse_err(
                    "could not parse git rev-parse --is-bare-repository output",
                ))
            }
        };
        let git_dir = lines
            .next()
            .ok_or_else(|| parse_err("git rev-parse did not report an absolute git dir"))?;

        // `--show-toplevel` fails outright in a bare repository (there is
        // no working tree), so it is only requested for non-bare
        // repositories.
        let root_path = if is_bare {
            PathBuf::from(git_dir)
        } else {
            let toplevel_output = self.run(
                vec![
                    "rev-parse".to_string(),
                    "--path-format=absolute".to_string(),
                    "--show-toplevel".to_string(),
                ],
                path,
            )?;
            let toplevel = Self::stdout_string(&toplevel_output)?;
            PathBuf::from(toplevel.trim())
        };

        let worktree_path = if is_bare {
            None
        } else {
            Some(root_path.clone())
        };
        let id = RepositoryId::from_canonical_root(&root_path);
        let head_state = self.determine_head_state(path)?;
        let current_branch = match &head_state {
            HeadState::Attached { branch } => Some(branch.clone()),
            HeadState::Detached { .. } | HeadState::Unborn => None,
        };

        Ok(Repository {
            id,
            root_path,
            worktree_path,
            is_bare,
            head_state,
            current_branch,
        })
    }

    fn status(&self, repo: &Repository) -> Result<RepositoryStatus, GitSailError> {
        require_worktree(repo, "status")?;
        let output = self.run(
            vec![
                "status".to_string(),
                "--porcelain=v2".to_string(),
                "--branch".to_string(),
                "-z".to_string(),
            ],
            &repo.root_path,
        )?;
        let stdout = Self::stdout_string(&output)?;
        parse_status(&stdout)
    }

    fn commits(
        &self,
        repo: &Repository,
        query: &CommitQuery,
    ) -> Result<Page<Commit>, GitSailError> {
        let limit = query.limit.unwrap_or(DEFAULT_COMMIT_LIMIT);
        if limit == 0 {
            return Err(parse_err("commit query limit must be greater than zero"));
        }
        let offset: u32 = match &query.cursor {
            None => 0,
            Some(cursor) => cursor
                .parse()
                .map_err(|_| parse_err("commit query cursor was not a valid offset"))?,
        };

        let mut args = vec![
            "log".to_string(),
            format!("--pretty=format:{LOG_FORMAT}"),
            "--date=raw".to_string(),
            "--no-color".to_string(),
            "-n".to_string(),
            (u64::from(limit) + 1).to_string(),
            format!("--skip={offset}"),
        ];
        if let Some(author) = &query.author {
            args.push(format!("--author={author}"));
        }
        if let Some(text_query) = &query.text_query {
            args.push(format!("--grep={text_query}"));
        }
        if query.path_filter.is_some() && query.follow_renames {
            // `--follow` only makes sense (and is only accepted by Git)
            // together with a single pathspec, which `path_filter` already
            // guarantees. Pushed before `--end-of-options` below: every real
            // option must precede it, or Git refuses with "option ... must
            // come before non-option arguments".
            args.push("--follow".to_string());
        }
        let revision = query
            .branch
            .as_ref()
            .map(BranchName::as_str)
            .or(query.revision_range.as_deref())
            .unwrap_or("HEAD");
        // `query.revision_range` (and, in principle, a branch name) is
        // caller-controlled free text that must never be interpreted as a
        // `git log` option (EPIC-22/US-110 criterion 1): a value crafted to
        // look like a flag (e.g. `--output=...`) must fail as an
        // unresolvable revision, not silently change what this invocation
        // does. Plain `--` cannot be used here because `git log` treats a
        // trailing `--` as the revision/pathspec boundary, not an
        // options/positional boundary; `--end-of-options` (supported since
        // Git 2.24) is the argument Git itself provides for exactly this.
        args.push("--end-of-options".to_string());
        args.push(revision.to_string());
        if let Some(path) = &query.path_filter {
            args.push("--".to_string());
            args.push(path.to_string_lossy().into_owned());
        }

        // A `git log` that cannot resolve `revision` (e.g. `HEAD` on an
        // unborn branch) is treated as an empty page rather than an error:
        // it is a legitimate repository state, not a failure.
        let Some(output) = self.try_run(args, &repo.root_path)? else {
            return Ok(Page {
                items: Vec::new(),
                next_cursor: None,
                has_more: false,
            });
        };
        let stdout = Self::stdout_string(&output)?;
        let mut items = parse_log_records(&stdout)?;

        let has_more = items.len() > limit as usize;
        if has_more {
            items.truncate(limit as usize);
        }
        let next_cursor = has_more.then(|| (offset + limit).to_string());

        Ok(Page {
            items,
            next_cursor,
            has_more,
        })
    }

    fn commit(&self, repo: &Repository, hash: &CommitHash) -> Result<Commit, GitSailError> {
        let args = vec![
            "log".to_string(),
            "-1".to_string(),
            format!("--pretty=format:{LOG_FORMAT}"),
            "--date=raw".to_string(),
            "--no-color".to_string(),
            hash.as_str().to_string(),
        ];
        let output = self.try_run(args, &repo.root_path)?.ok_or_else(|| {
            GitSailError::new(
                ErrorCode::RepositoryNotFound,
                format!("no commit found for {hash}"),
            )
        })?;
        let stdout = Self::stdout_string(&output)?;
        let mut commits = parse_log_records(&stdout)?;
        if commits.is_empty() {
            return Err(GitSailError::new(
                ErrorCode::RepositoryNotFound,
                format!("no commit found for {hash}"),
            ));
        }
        Ok(commits.remove(0))
    }

    fn branches(&self, repo: &Repository) -> Result<Vec<Branch>, GitSailError> {
        let args = vec![
            "for-each-ref".to_string(),
            format!("--format={FOR_EACH_REF_FORMAT}"),
            "refs/heads".to_string(),
            "refs/remotes".to_string(),
        ];
        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;

        let mut branches = Vec::new();
        for line in stdout.lines() {
            if line.is_empty() {
                continue;
            }
            if let Some(branch) = parse_ref_line(line)? {
                branches.push(branch);
            }
        }
        Ok(branches)
    }

    fn diff(
        &self,
        repo: &Repository,
        request: &DiffRequest,
        cancel: &CancellationToken,
    ) -> Result<Diff, GitSailError> {
        // `from`/`to` of `None` resolve to the working tree/index (see
        // `DiffRequest`'s docs), and `staged` compares the index itself;
        // none of those exist in a bare repository.
        if request.staged || request.from.is_none() || request.to.is_none() {
            require_worktree(repo, "diff")?;
        }
        if request.staged && request.to.is_some() {
            return Err(parse_err(
                "DiffRequest::to must be None when staged is true (git diff --cached compares the index against a single tree)",
            ));
        }
        let context_lines = request.context_lines.unwrap_or(3);
        let mut args = vec![
            // Disabled as a global option (must precede the subcommand):
            // without it, Git C-style-quotes any path byte >= 0x80 (e.g.
            // Unicode) in the `---`/`+++`/`diff --git` header lines this
            // adapter parses, which this parser does not unescape (US-008).
            "-c".to_string(),
            "core.quotePath=false".to_string(),
            "diff".to_string(),
            "--no-color".to_string(),
            "--no-ext-diff".to_string(),
            "-M".to_string(),
            format!("-U{context_lines}"),
        ];
        if request.staged {
            args.push("--cached".to_string());
        }
        if let Some(from) = &request.from {
            args.push(from.as_str().to_string());
        }
        if let Some(to) = &request.to {
            args.push(to.as_str().to_string());
        }
        if let Some(path_filter) = &request.path_filter {
            args.push("--".to_string());
            args.push(path_filter.to_string_lossy().into_owned());
        }

        let output = self.run_cancellable(args, &repo.root_path, cancel)?;
        let stdout = Self::stdout_string(&output)?;
        parse_diff(&stdout, cancel)
    }

    fn resolve_revision(
        &self,
        repo: &Repository,
        revision: &str,
    ) -> Result<CommitHash, GitSailError> {
        require_worktree(repo, "resolve revision")?;
        // `^{commit}` peels tags/other objects down to a commit and makes
        // `rev-parse` fail for anything that does not resolve to one, so a
        // caller always gets an unambiguous commit or a clear error rather
        // than a tree/blob id it cannot diff against (US-028 criterion 1).
        //
        // `revision` is caller-controlled free text (EPIC-22/US-110
        // criterion 1) — e.g. `gitsail`'s own `--revision`/positional CLI
        // arguments, or a TUI/Desktop text field — and must never be
        // interpreted as a `rev-parse` option. `--end-of-options` (not plain
        // `--`, which puts `rev-parse` into path-only mode and would make
        // every revision fail to resolve) forces everything after it to be
        // treated as a non-option argument, so a crafted value that looks
        // like a flag fails as an unresolvable revision instead of being
        // parsed as one.
        let output = self.try_run(
            vec![
                "rev-parse".to_string(),
                "--verify".to_string(),
                "-q".to_string(),
                "--end-of-options".to_string(),
                format!("{revision}^{{commit}}"),
            ],
            &repo.root_path,
        )?;
        let output = output.ok_or_else(|| {
            GitSailError::new(
                ErrorCode::RepositoryNotFound,
                format!("revision '{revision}' could not be resolved to a commit"),
            )
            .with_remediation("use a valid branch, tag, or commit reference")
        })?;
        let hash = Self::stdout_string(&output)?.trim().to_string();
        CommitHash::new(hash)
    }

    fn blame(
        &self,
        repo: &Repository,
        request: &BlameRequest,
        cancel: &CancellationToken,
    ) -> Result<Blame, GitSailError> {
        if request.buffer_contents.is_some() && request.revision.is_some() {
            return Err(parse_err(
                "BlameRequest::buffer_contents is only meaningful when revision is None (git blame --contents blames the working tree)",
            ));
        }
        if let Some(range) = &request.line_range {
            if !range.is_valid() {
                return Err(parse_err(format!(
                    "invalid line range {}..={}: start must be at least 1 and not greater than end",
                    range.start, range.end
                )));
            }
        }
        if request.revision.is_none() {
            require_worktree(repo, "blame")?;
        }

        let mut args = vec!["blame".to_string(), "--porcelain".to_string()];
        if let Some(range) = &request.line_range {
            args.push("-L".to_string());
            args.push(format!("{},{}", range.start, range.end));
        }
        if request.buffer_contents.is_some() {
            args.push("--contents".to_string());
            args.push("-".to_string());
        }
        if let Some(rev) = &request.revision {
            args.push(rev.as_str().to_string());
        }
        args.push("--".to_string());
        args.push(request.file.to_string_lossy().into_owned());

        let output = match &request.buffer_contents {
            Some(contents) => self
                .run_with_stdin_cancellable(args, &repo.root_path, contents.clone(), cancel)
                .map_err(classify_blame_failure)?,
            None => self
                .run_cancellable(args, &repo.root_path, cancel)
                .map_err(classify_blame_failure)?,
        };
        let stdout = Self::stdout_string(&output)?;
        let lines = parse_blame(&stdout, cancel)?;
        Ok(Blame {
            file: request.file.clone(),
            revision: request.revision.clone(),
            lines,
        })
    }

    /// Traces the commit-level history of `request.range` in `request.file`
    /// via `git log -L` (US-019). Unlike `blame`, this always resolves to a
    /// real commit — there is no working-tree mode — so `revision: None`
    /// resolves explicitly to `HEAD` up front (criterion 3: uncommitted
    /// content must never receive a misleading commit attribution).
    ///
    /// Known limitation inherited from `git log -L<range>:<path>`'s own
    /// syntax: a `path` containing a colon cannot be expressed this way:
    /// this is a Git CLI limitation, not one introduced by this adapter.
    fn line_history(
        &self,
        repo: &Repository,
        request: &LineHistoryRequest,
        cancel: &CancellationToken,
    ) -> Result<LineHistory, GitSailError> {
        if !request.range.is_valid() {
            return Err(parse_err(format!(
                "invalid line range {}..={}: start must be at least 1 and not greater than end",
                request.range.start, request.range.end
            )));
        }
        // `request.revision` is already a resolved commit (mirroring
        // `BlameRequest`): only a `None` default needs resolving, to echo
        // back the exact commit `HEAD` named rather than the literal string
        // "HEAD" (SAD's "echo back what was queried" convention, US-033).
        let resolved_revision = match &request.revision {
            Some(hash) => hash.clone(),
            None => self.resolve_revision(repo, "HEAD")?,
        };
        let revision_arg = resolved_revision.as_str().to_string();

        let args = vec![
            "log".to_string(),
            format!(
                "-L{},{}:{}",
                request.range.start,
                request.range.end,
                request.file.to_string_lossy()
            ),
            "--no-color".to_string(),
            "--pretty=format:%H%x1e".to_string(),
            revision_arg,
        ];
        let output = self
            .run_cancellable(args, &repo.root_path, cancel)
            .map_err(classify_line_history_failure)?;
        let stdout = Self::stdout_string(&output)?;
        let blocks = split_line_history_blocks(&stdout)?;

        let mut entries = Vec::with_capacity(blocks.len());
        for (hash, diff_lines) in blocks {
            let commit_hash = CommitHash::new(hash)?;
            let commit = self.commit(repo, &commit_hash)?;
            let hunks = parse_line_history_hunks(&diff_lines)?;
            entries.push(LineHistoryEntry { commit, hunks });
        }

        Ok(LineHistory {
            file: request.file.clone(),
            revision: resolved_revision,
            range: request.range,
            entries,
        })
    }

    /// Reads `path`'s full content as of `revision` via `git show
    /// <revision>:<path>` (EPIC-15/US-076) — a single positional argument,
    /// colon-joined, exactly as `git show` itself expects; this cannot be
    /// split into separate arguments or paired with a `--` separator without
    /// changing what Git parses it as.
    ///
    /// The caller is expected to have already resolved `revision` to a real
    /// commit (via `resolve_revision`), mirroring `commit`/`line_history`'s
    /// own convention, so a non-zero exit here can only mean "path not found
    /// in that tree" and is reported as [`FileContentKind::Missing`] rather
    /// than an error. Content is classified as [`FileContentKind::Binary`]
    /// when a NUL byte appears in its first 8000 bytes or it is not valid
    /// UTF-8 — otherwise it is [`FileContentKind::Text`].
    fn file_content(
        &self,
        repo: &Repository,
        revision: &CommitHash,
        path: &Path,
    ) -> Result<FileContentAtRevision, GitSailError> {
        let object = format!("{}:{}", revision.as_str(), path.to_string_lossy());
        let args = vec!["show".to_string(), object];

        let kind = match self.try_run(args, &repo.root_path)? {
            None => FileContentKind::Missing,
            Some(output) => classify_file_content(&output.stdout),
        };

        Ok(FileContentAtRevision {
            path: path.to_path_buf(),
            revision: revision.clone(),
            kind,
        })
    }

    /// Resolves the real, shared `.git` directory via `git rev-parse
    /// --git-common-dir` (ADR-019; T-227/US-116 criterion 1), so two linked
    /// worktrees of the same repository — which report distinct
    /// [`Repository::root_path`]s but share one object database/refs/index
    /// lock namespace — resolve to the same mutation-serialization lock key
    /// instead of two independent ones. Falls back to `--absolute-git-dir`
    /// for older Git versions that lack `--git-common-dir` (added in Git
    /// 2.5), which is at least correct for a repository with no linked
    /// worktrees (the common case).
    fn lock_key(&self, repo: &Repository) -> Result<PathBuf, GitSailError> {
        let cwd = repo.worktree_path.as_deref().unwrap_or(&repo.root_path);
        let args = vec![
            "rev-parse".to_string(),
            "--path-format=absolute".to_string(),
            "--git-common-dir".to_string(),
        ];
        let output = match self.try_run(args, cwd)? {
            Some(output) => output,
            None => self
                .try_run(
                    vec![
                        "rev-parse".to_string(),
                        "--path-format=absolute".to_string(),
                        "--absolute-git-dir".to_string(),
                    ],
                    cwd,
                )?
                .ok_or_else(|| {
                    GitSailError::new(
                        ErrorCode::RepositoryNotFound,
                        "could not resolve the repository's Git directory",
                    )
                })?,
        };
        let stdout = Self::stdout_string(&output)?;
        let path = stdout
            .lines()
            .next()
            .ok_or_else(|| parse_err("git rev-parse did not report a Git common directory"))?;
        Ok(PathBuf::from(path))
    }

    /// Lists local tags via `git for-each-ref refs/tags` (EPIC-18/T-216/
    /// US-091 criterion 1). An empty result (no tags at all) is a valid
    /// state, not an error.
    fn list_tags(&self, repo: &Repository) -> Result<Vec<Tag>, GitSailError> {
        let args = vec![
            "for-each-ref".to_string(),
            format!("--format={TAG_FORMAT}"),
            "refs/tags".to_string(),
        ];
        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        parse_tag_records(&stdout)
    }

    /// Lists configured remotes (EPIC-18/T-216/US-091 criterion 1),
    /// resolving each name's fetch and push URL independently via `git
    /// remote get-url`/`get-url --push` rather than parsing `git remote
    /// -v`'s `name\turl (fetch|push)` text — Git itself already
    /// distinguishes the two per name this way, including the case where
    /// `--push` was never configured separately (it then simply reports the
    /// same URL as fetch), and this avoids parsing a name or URL that could
    /// otherwise be confused with the trailing `(fetch)`/`(push)` marker.
    /// Embedded credentials are never stripped here: [`RemoteUrl`] itself
    /// only ever renders its `redacted()` form via `Display`/`Debug`, so any
    /// diagnostic built from this stays safe (US-091 criterion 3) while the
    /// raw URL remains available via `RemoteUrl::as_str` for anything that
    /// genuinely needs to invoke Git with it.
    fn list_remotes(&self, repo: &Repository) -> Result<Vec<Remote>, GitSailError> {
        let names_output = self.run(vec!["remote".to_string()], &repo.root_path)?;
        let names_stdout = Self::stdout_string(&names_output)?;

        let mut remotes = Vec::new();
        for name in names_stdout.lines().filter(|l| !l.is_empty()) {
            let fetch_output = self.run(
                vec![
                    "remote".to_string(),
                    "get-url".to_string(),
                    name.to_string(),
                ],
                &repo.root_path,
            )?;
            let fetch_url = Self::stdout_string(&fetch_output)?.trim().to_string();

            let push_output = self.run(
                vec![
                    "remote".to_string(),
                    "get-url".to_string(),
                    "--push".to_string(),
                    name.to_string(),
                ],
                &repo.root_path,
            )?;
            let push_url = Self::stdout_string(&push_output)?.trim().to_string();

            remotes.push(Remote {
                name: name.to_string(),
                fetch_url: RemoteUrl::new(fetch_url),
                push_url: RemoteUrl::new(push_url),
            });
        }
        Ok(remotes)
    }

    /// Lists stash entries, newest (`stash@{0}`) first (US-091 criterion 2).
    /// See [`STASH_FORMAT`]'s doc for why the index is this record's
    /// position rather than a parsed `%gd`.
    fn list_stash_entries(&self, repo: &Repository) -> Result<Vec<Stash>, GitSailError> {
        let args = vec![
            "stash".to_string(),
            "list".to_string(),
            "--date=raw".to_string(),
            format!("--format={STASH_FORMAT}"),
        ];
        // No stash ref exists at all until the first `stash push` — `git
        // stash list` still exits 0 with empty output in that case, so this
        // never needs `try_run`'s "treat a failure as empty" fallback; a
        // genuine failure here is a real error.
        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        parse_stash_records(&stdout)
    }

    /// Lists worktrees via `git worktree list --porcelain -z` (US-095
    /// criterion 1). `-z` (NUL-delimited, block-separated by an empty
    /// field) is used instead of the newline-delimited default so a path
    /// containing an embedded newline round-trips correctly.
    fn list_worktrees(&self, repo: &Repository) -> Result<Vec<Worktree>, GitSailError> {
        let args = vec![
            "worktree".to_string(),
            "list".to_string(),
            "--porcelain".to_string(),
            "-z".to_string(),
        ];
        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        parse_worktree_records(&stdout)
    }
}

impl RepositoryWritePort for GitCliProvider {
    fn stage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        require_worktree(repo, "stage")?;
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["add".to_string(), "--".to_string()];
        args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
        self.run(args, &repo.root_path)?;
        Ok(())
    }

    fn unstage_files(&self, repo: &Repository, paths: &[PathBuf]) -> Result<(), GitSailError> {
        require_worktree(repo, "unstage")?;
        if paths.is_empty() {
            return Ok(());
        }

        // Before the first commit, there is no HEAD for `git restore
        // --staged` to read from (it fails with "could not resolve HEAD");
        // `git rm --cached` unstages by editing the index directly and
        // works regardless of HEAD, leaving the working tree untouched.
        if matches!(
            self.determine_head_state(&repo.root_path)?,
            HeadState::Unborn
        ) {
            let mut args = vec![
                "rm".to_string(),
                "--cached".to_string(),
                "-q".to_string(),
                "--".to_string(),
            ];
            args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
            self.run(args, &repo.root_path)?;
            return Ok(());
        }

        let expanded = self.expand_rename_pairs(repo, paths)?;
        let mut args = vec![
            "restore".to_string(),
            "--staged".to_string(),
            "--".to_string(),
        ];
        args.extend(expanded.iter().map(|p| p.to_string_lossy().into_owned()));
        self.run(args, &repo.root_path)?;
        Ok(())
    }

    fn create_commit(&self, repo: &Repository, message: &str) -> Result<CommitHash, GitSailError> {
        require_worktree(repo, "commit")?;
        if message.trim().is_empty() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "commit message must not be empty",
            )
            .with_remediation("provide a non-empty commit message"));
        }
        // Checked up front rather than left to `git commit`'s own refusal:
        // that message ("nothing to commit, working tree clean") goes to
        // stdout, not stderr, so it would never reach this error's
        // diagnostic (only stderr is captured there). `status()` is
        // already exercised and gives an unambiguous answer (US-012
        // criterion 1: never create an empty commit implicitly).
        let status = RepositoryReadPort::status(self, repo)?;
        let has_staged_changes = status
            .files
            .iter()
            .any(|f| f.index_status != FileStatusCode::Unmodified);
        if !has_staged_changes {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "nothing staged to commit",
            )
            .with_remediation("stage changes before committing"));
        }

        let args = vec!["commit".to_string(), "-m".to_string(), message.to_string()];
        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let hash_output = self.run(
                    vec!["rev-parse".to_string(), "HEAD".to_string()],
                    &repo.root_path,
                )?;
                let hash = Self::stdout_string(&hash_output)?.trim().to_string();
                CommitHash::new(hash)
            }
            Err(err) => Err(classify_commit_failure(err)),
        }
    }

    fn stage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        require_worktree(repo, "stage hunks")?;
        self.apply_hunk_selection(repo, selection, ApplyDirection::Forward)
    }

    fn unstage_hunks(&self, repo: &Repository, selection: &[FileDiff]) -> Result<(), GitSailError> {
        require_worktree(repo, "unstage hunks")?;
        self.apply_hunk_selection(repo, selection, ApplyDirection::Reverse)
    }

    fn switch_branch(&self, repo: &Repository, target: &BranchName) -> Result<(), GitSailError> {
        require_worktree(repo, "switch branch")?;
        // `--` ends option parsing before `target` (EPIC-22/US-110 criterion
        // 1): `git switch` accepts it, so a caller-supplied name that
        // happens to look like a flag (e.g. `-f`, `--force`, one of
        // `switch`'s own real options) is never parsed as one — it is
        // rejected as an invalid branch name instead, exactly as any other
        // unresolvable target would be.
        let args = vec![
            "switch".to_string(),
            "--".to_string(),
            target.as_str().to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_switch_failure)?;
        Ok(())
    }

    fn create_branch(
        &self,
        repo: &Repository,
        name: &BranchName,
        start_point: Option<&CommitHash>,
    ) -> Result<(), GitSailError> {
        // `--` ends option parsing before `name`/`start_point` (same
        // rationale as `switch_branch` above); `git branch` accepts it, and
        // `start_point` (a hex-validated `CommitHash`) can never itself look
        // like a flag, but keeping both positionals after the same `--`
        // is simplest and matches how a person would type this on a shell.
        let mut args = vec![
            "branch".to_string(),
            "--".to_string(),
            name.as_str().to_string(),
        ];
        if let Some(start) = start_point {
            args.push(start.as_str().to_string());
        }
        self.run(args, &repo.root_path)
            .map_err(classify_create_branch_failure)?;
        Ok(())
    }

    fn delete_branch(
        &self,
        repo: &Repository,
        name: &BranchName,
        force: bool,
    ) -> Result<(), GitSailError> {
        let flag = if force { "-D" } else { "-d" };
        // `--` ends option parsing before `name` (same rationale as
        // `switch_branch`/`create_branch` above).
        let args = vec![
            "branch".to_string(),
            flag.to_string(),
            "--".to_string(),
            name.as_str().to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_delete_branch_failure)?;
        Ok(())
    }

    fn amend_commit(
        &self,
        repo: &Repository,
        message: &str,
        expected_head: &CommitHash,
    ) -> Result<CommitHash, GitSailError> {
        require_worktree(repo, "amend")?;
        if message.trim().is_empty() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "commit message must not be empty",
            )
            .with_remediation("provide a non-empty commit message"));
        }
        // Revalidated immediately before the mutation, mirroring
        // `create_commit`'s pre-flight `status()` check just above: a
        // confirmation given against an older `HEAD` (from a preview read)
        // must never authorize amending whatever commit happens to be
        // `HEAD` *now* (US-059 criterion 3). Built on
        // `gitsail_application::Precondition` (EPIC-22/T-222/US-111) rather
        // than a bespoke equality check, so this is the same mechanism any
        // future mutation with the same preview/execute race window reuses.
        let current_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
        gitsail_application::Precondition::new(expected_head.clone())
            .revalidate(&current_head)
            .map_err(|_| {
                GitSailError::new(
                    ErrorCode::OperationConflict,
                    "HEAD changed since the amend was previewed",
                )
                .with_remediation(
                    "review the new HEAD commit and retry the amend if it is still what you intend to change",
                )
            })?;

        let args = vec![
            "commit".to_string(),
            "--amend".to_string(),
            "-m".to_string(),
            message.to_string(),
        ];
        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let hash_output = self.run(
                    vec!["rev-parse".to_string(), "HEAD".to_string()],
                    &repo.root_path,
                )?;
                let hash = Self::stdout_string(&hash_output)?.trim().to_string();
                CommitHash::new(hash)
            }
            Err(err) => Err(classify_commit_failure(err)),
        }
    }

    /// Creates a new stash via `git stash push` (US-092). `scope`'s flags
    /// are passed through 1:1 (this adapter never infers or hides one), and
    /// this always reports whether anything was actually captured rather
    /// than trusting `git stash push`'s own "No local changes to save"
    /// wording (locale-independent and format-stable either way): the stash
    /// list's length before and after is compared instead, and the newest
    /// entry (`stash@{0}`) is returned on success (US-092 criterion 3).
    fn create_stash(
        &self,
        repo: &Repository,
        message: Option<&str>,
        scope: StashScope,
    ) -> Result<Stash, GitSailError> {
        require_worktree(repo, "stash")?;
        let before = RepositoryReadPort::list_stash_entries(self, repo)?.len();

        let mut args = vec!["stash".to_string(), "push".to_string()];
        if scope.keep_index {
            args.push("--keep-index".to_string());
        }
        if scope.all {
            // `--all` already implies capturing untracked files too, so
            // `--include-untracked` is never also passed here (Git accepts
            // both together, but that would be redundant, not a behavior
            // change).
            args.push("--all".to_string());
        } else if scope.include_untracked {
            args.push("--include-untracked".to_string());
        }
        if let Some(message) = message {
            args.push("-m".to_string());
            args.push(message.to_string());
        }

        self.run(args, &repo.root_path)?;

        let after = RepositoryReadPort::list_stash_entries(self, repo)?;
        if after.len() == before {
            return Err(
                GitSailError::new(ErrorCode::InvalidRepositoryState, "nothing to stash")
                    .with_remediation(
                        "make some changes within the requested scope before stashing",
                    ),
            );
        }
        // `git stash push` can only ever grow the stash list by exactly one
        // (it never reorders or removes existing entries), so the new
        // entry is always the newest one, `stash@{0}`.
        Ok(after
            .into_iter()
            .next()
            .expect("just checked after.len() > before >= 0"))
    }

    /// Applies `expected` via `git stash apply` without removing it
    /// (US-093 criterion 1). See [`Self::revalidate_stash_identity`] for the
    /// precondition check run first, and [`classify_stash_restore_outcome`]
    /// for how a conflict is distinguished from a genuine failure.
    fn apply_stash(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        require_worktree(repo, "stash apply")?;
        self.revalidate_stash_identity(repo, expected)?;
        let stash_ref = format!("stash@{{{}}}", expected.index);
        let args = vec!["stash".to_string(), "apply".to_string(), stash_ref];
        match self.run(args, &repo.root_path) {
            Ok(_) => Ok(StashApplyOutcome {
                had_conflicts: false,
            }),
            Err(err) => classify_stash_restore_outcome(err),
        }
    }

    /// Applies `expected`, then removes it — but only when the apply
    /// succeeded without conflicts (US-093 criterion 2). `git stash pop`
    /// already refuses to drop the entry itself when the apply half
    /// conflicts (verified directly against real Git: a conflicted `pop`
    /// exits non-zero and prints "The stash entry is kept in case you need
    /// it again"), so this method does not need its own extra bookkeeping
    /// to avoid dropping on conflict — it only needs to *report* the
    /// conflict accurately rather than as a plain success, which
    /// [`classify_stash_restore_outcome`] already does identically to
    /// [`Self::apply_stash`].
    fn pop_stash(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<StashApplyOutcome, GitSailError> {
        require_worktree(repo, "stash pop")?;
        self.revalidate_stash_identity(repo, expected)?;
        let stash_ref = format!("stash@{{{}}}", expected.index);
        let args = vec!["stash".to_string(), "pop".to_string(), stash_ref];
        match self.run(args, &repo.root_path) {
            Ok(_) => Ok(StashApplyOutcome {
                had_conflicts: false,
            }),
            Err(err) => classify_stash_restore_outcome(err),
        }
    }

    /// Deletes `expected` via `git stash drop`, without applying it
    /// (US-093 criterion 3; `Destructive` — see
    /// [`gitsail_application::MutationKind::DropStash`]).
    fn drop_stash(&self, repo: &Repository, expected: &Stash) -> Result<(), GitSailError> {
        self.revalidate_stash_identity(repo, expected)?;
        let stash_ref = format!("stash@{{{}}}", expected.index);
        self.run(
            vec!["stash".to_string(), "drop".to_string(), stash_ref],
            &repo.root_path,
        )
        .map_err(classify_stash_missing_failure)?;
        Ok(())
    }

    /// Creates a local tag via `git tag` (US-094 criterion 1). Never passes
    /// `-f`: Git's own default refusal on a name collision is preserved
    /// (US-094 criterion 1), reclassified by
    /// [`classify_create_tag_failure`] into a clearer error than a bare
    /// process failure. Never contacts a remote (US-094 criterion 3) — `git
    /// tag` itself has no network effect.
    fn create_tag(
        &self,
        repo: &Repository,
        name: &str,
        target: Option<&CommitHash>,
        annotation: TagAnnotation,
    ) -> Result<(), GitSailError> {
        let mut args = vec!["tag".to_string()];
        if let TagAnnotation::Annotated { message } = &annotation {
            args.push("-a".to_string());
            args.push("-m".to_string());
            args.push(message.clone());
        }
        // `--` ends option parsing before `name`/`target` (same rationale as
        // `create_branch`'s own `--`): a caller-supplied tag name that
        // happens to look like a flag is rejected as an invalid tag name
        // rather than parsed as one.
        args.push("--".to_string());
        args.push(name.to_string());
        if let Some(target) = target {
            args.push(target.as_str().to_string());
        }
        self.run(args, &repo.root_path)
            .map_err(classify_create_tag_failure)?;
        Ok(())
    }

    /// Deletes the local tag `name` via `git tag -d` (US-094 criterion 2).
    /// Always local (US-094 criterion 3).
    fn delete_tag(&self, repo: &Repository, name: &str) -> Result<(), GitSailError> {
        let args = vec![
            "tag".to_string(),
            "-d".to_string(),
            "--".to_string(),
            name.to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_delete_tag_failure)?;
        Ok(())
    }

    /// Creates a new worktree via `git worktree add` (US-095 criterion 2).
    /// Propagates Git's own refusal, reclassified by
    /// [`classify_create_worktree_failure`], when `path` already exists
    /// unexpectedly or `branch` names a branch already checked out
    /// elsewhere — this never works around either (US-095 criterion 2:
    /// "propague esse erro com clareza, não tente contornar").
    fn create_worktree(
        &self,
        repo: &Repository,
        path: &Path,
        branch: WorktreeBranchSpec,
    ) -> Result<Worktree, GitSailError> {
        require_worktree(repo, "create worktree")?;
        let path_arg = path.to_string_lossy().into_owned();
        let mut args = vec!["worktree".to_string(), "add".to_string()];
        match &branch {
            WorktreeBranchSpec::ExistingBranch(name) => {
                args.push(path_arg);
                args.push(name.as_str().to_string());
            }
            WorktreeBranchSpec::NewBranch { name, start_point } => {
                args.push("-b".to_string());
                args.push(name.as_str().to_string());
                args.push(path_arg);
                if let Some(start) = start_point {
                    args.push(start.as_str().to_string());
                }
            }
            WorktreeBranchSpec::Detached(commit) => {
                args.push("--detach".to_string());
                args.push(path_arg);
                args.push(commit.as_str().to_string());
            }
        }
        self.run(args, &repo.root_path)
            .map_err(classify_create_worktree_failure)?;

        // `git worktree add` reports success without echoing back a
        // machine-readable description of what it created, so the new
        // worktree is located by re-listing rather than assembled from the
        // request alone (which would not know, e.g., the resolved detached
        // HEAD commit for a `NewBranch { start_point: None }` request).
        // Matched by canonical path where possible (Git reports the
        // worktree's real, canonicalized path, which is not always
        // byte-identical to the caller's `path`, e.g. a symlinked temp
        // directory) — falling back to a literal match otherwise, a known
        // simplification for a path that cannot be canonicalized (e.g. does
        // not exist, on a filesystem quirk).
        let canonical_requested = std::fs::canonicalize(path).ok();
        let worktrees = RepositoryReadPort::list_worktrees(self, repo)?;
        worktrees
            .into_iter()
            .find(|w| Some(&w.path) == canonical_requested.as_ref() || w.path == path)
            .ok_or_else(|| {
                parse_err(
                    "worktree add succeeded but the new worktree could not be found in the listing",
                )
            })
    }

    /// Removes the worktree at `path` via `git worktree remove` (US-095
    /// criterion 3). With `force: false`, propagates Git's own refusal
    /// (reclassified by [`classify_remove_worktree_failure`]) when the
    /// worktree has uncommitted changes, rather than discarding them
    /// implicitly. Deliberately never escalates to Git's own "remove a
    /// locked worktree" double-force (`-f -f`): a locked worktree's lock is
    /// left untouched, surfaced as a clear, classified error instead (not
    /// an "advanced workspace manager" concern this story's scope covers).
    fn remove_worktree(
        &self,
        repo: &Repository,
        path: &Path,
        force: bool,
    ) -> Result<(), GitSailError> {
        let mut args = vec!["worktree".to_string(), "remove".to_string()];
        if force {
            args.push("--force".to_string());
        }
        args.push(path.to_string_lossy().into_owned());
        self.run(args, &repo.root_path)
            .map_err(classify_remove_worktree_failure)?;
        Ok(())
    }
}

/// Direction in which a reconstructed hunk patch is applied to the index:
/// `Forward` moves unstaged hunks into the index (stage), `Reverse` removes
/// staged hunks from the index without touching the working tree (unstage).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApplyDirection {
    Forward,
    Reverse,
}

impl GitCliProvider {
    /// Runs `args` in `cwd` with `stdin` piped in, pinning the `C` locale
    /// like every other invocation (SAD §11).
    fn run_with_stdin(
        &self,
        args: Vec<String>,
        cwd: &Path,
        stdin: Vec<u8>,
    ) -> Result<ProcessOutput, GitSailError> {
        self.run_with_stdin_cancellable(args, cwd, stdin, &CancellationToken::new())
    }

    /// Like [`Self::run_with_stdin`], but forwards a caller-supplied
    /// cancellation token instead of a fresh, never-cancelled one (used by
    /// `blame` when [`BlameRequest::buffer_contents`] is set, US-034
    /// criterion 3).
    fn run_with_stdin_cancellable(
        &self,
        args: Vec<String>,
        cwd: &Path,
        stdin: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<ProcessOutput, GitSailError> {
        let request = ProcessRequest::new(args, cwd.to_path_buf())
            .with_env(Self::locale_env())
            .with_stdin(stdin);
        self.runner.run(request, cancel)
    }

    /// Expands `paths` so that unstaging a rename by only its new path (or
    /// only its old path) still fully unstages both sides: `git restore
    /// --staged <new-path>` alone leaves the paired removal of the old path
    /// staged, which is a half-unstage, not a full one.
    fn expand_rename_pairs(
        &self,
        repo: &Repository,
        paths: &[PathBuf],
    ) -> Result<Vec<PathBuf>, GitSailError> {
        let status = RepositoryReadPort::status(self, repo)?;
        let mut expanded = Vec::with_capacity(paths.len());
        for path in paths {
            expanded.push(path.clone());
            let renamed_pair = status.files.iter().find(|f| &f.path == path).and_then(|f| {
                matches!(f.change_type, ChangeType::Renamed | ChangeType::Copied)
                    .then(|| f.previous_path.clone())
                    .flatten()
            });
            if let Some(previous_path) = renamed_pair {
                expanded.push(previous_path);
            }
        }
        Ok(expanded)
    }

    /// Renders `selection` as a `git apply`-compatible unified diff and
    /// applies it to the index only (`--cached`), never the working tree
    /// (US-013 criterion 2). `git apply` itself refuses a patch whose
    /// context no longer matches the current index (US-013 criterion 3:
    /// "diff obsoleto ou hunk inaplicável impede aplicação cega"); that
    /// failure is remapped to `OperationConflict` here rather than left as
    /// an opaque process failure.
    fn apply_hunk_selection(
        &self,
        repo: &Repository,
        selection: &[FileDiff],
        direction: ApplyDirection,
    ) -> Result<(), GitSailError> {
        if selection.iter().all(|f| f.hunks.is_empty()) {
            return Ok(());
        }
        if let Some(binary) = selection.iter().find(|f| f.is_binary) {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "hunk-level staging is not supported for binary file {}",
                    binary.path.display()
                ),
            )
            .with_remediation("stage the whole file instead"));
        }
        if let Some(renamed) = selection
            .iter()
            .find(|f| matches!(f.change_type, ChangeType::Renamed | ChangeType::Copied))
        {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "hunk-level staging is not supported for renamed/copied file {}",
                    renamed.path.display()
                ),
            )
            .with_remediation("stage the whole file instead"));
        }

        let patch = render_hunk_patch(selection);
        let mut args = vec![
            "apply".to_string(),
            "--cached".to_string(),
            "--whitespace=nowarn".to_string(),
        ];
        if direction == ApplyDirection::Reverse {
            args.push("--reverse".to_string());
        }
        args.push("-".to_string());

        self.run_with_stdin(args, &repo.root_path, patch.into_bytes())
            .map_err(classify_apply_failure)?;
        Ok(())
    }

    /// Revalidates `expected` (a stash entry a caller observed via a prior
    /// [`RepositoryReadPort::list_stash_entries`] read) is still the same
    /// entry at the same stack position immediately before
    /// `apply_stash`/`pop_stash`/`drop_stash` act on it (US-093 criterion
    /// 1), via [`gitsail_application::Precondition`] — the same mechanism
    /// [`RepositoryWritePort::amend_commit`] already applies to `HEAD`.
    /// Refuses with [`ErrorCode::OperationConflict`] both when the entry at
    /// that position no longer matches (another entry was pushed/dropped,
    /// shifting indices) and when the stash list has since shrunk past that
    /// position entirely (T-218/US-093 DoD: "o índice do stash ter mudado
    /// externamente entre a prévia e a confirmação").
    fn revalidate_stash_identity(
        &self,
        repo: &Repository,
        expected: &Stash,
    ) -> Result<(), GitSailError> {
        let current_entries = RepositoryReadPort::list_stash_entries(self, repo)?;
        match current_entries.into_iter().nth(expected.index as usize) {
            Some(current) => Precondition::new(expected.clone()).revalidate(&current),
            None => Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "stash@{{{}}} no longer exists (the stash list changed since it was previewed)",
                    expected.index
                ),
            )
            .with_remediation("refresh the stash list and retry against a current entry")),
        }
    }
}

/// Reclassifies a failed `git commit` when it failed for a missing
/// author/committer identity (US-012's "Falta de identidade ... gera
/// diagnóstico"), giving a clearer, actionable error than a bare process
/// failure. An empty index is rejected before `git commit` is even invoked
/// (see `create_commit`), so it is not handled here. Any other failure —
/// including a hook's own non-zero exit — passes through unchanged: its
/// stderr is already carried as this error's diagnostic, and the index is
/// untouched either way (git never partially applies a failed commit).
fn classify_commit_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    let identity_missing = diagnostic_text.contains("Please tell me who you are")
        || diagnostic_text.contains("no email was given")
        || diagnostic_text.contains("no name was given");

    if identity_missing {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "commit author identity is not configured",
        )
        .with_remediation("set git config user.name and user.email, then retry")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git apply --cached` as `OperationConflict` when
/// it failed because the patch's context no longer matches the index (a
/// stale diff or an already-modified hunk), so callers can tell "ask the
/// user to refresh and retry" apart from an unrelated process failure.
fn classify_apply_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    let stale = diagnostic_text.contains("patch does not apply")
        || diagnostic_text.contains("patch failed")
        || diagnostic_text.contains("does not match index");
    if stale {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "the selected hunk no longer applies to the current index",
        )
        .with_remediation("refresh the diff and reselect the hunks to stage/unstage")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git switch` when it failed because the switch
/// would overwrite local changes incompatible with the target branch
/// (US-021 criterion 2: "impede troca sem descarte implícito"), or because
/// the target does not resolve to a branch, giving a clearer, actionable
/// error than a bare process failure. Any other failure passes through
/// unchanged.
fn classify_switch_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("would be overwritten") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "switching branches would overwrite local changes",
        )
        .with_remediation("commit or stash your local changes before switching branches")
        .with_source(err)
    } else if diagnostic_text.contains("invalid reference") {
        GitSailError::new(ErrorCode::RepositoryNotFound, "no such branch")
            .with_remediation("verify the branch name")
            .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git branch <name> [<start-point>]` when it failed
/// because `name` already exists (US-022 criterion 2: "nome existente não é
/// sobrescrito") or `start-point` does not resolve to a commit. Any other
/// failure passes through unchanged.
fn classify_create_branch_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("already exists") {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "a branch with that name already exists",
        )
        .with_remediation("choose a different branch name")
        .with_source(err)
    } else if diagnostic_text.contains("not a valid object name") {
        GitSailError::new(ErrorCode::RepositoryNotFound, "start point does not exist")
            .with_remediation("choose an existing commit, branch or tag as the start point")
            .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git blame` as `RepositoryNotFound` when the
/// queried path does not exist or is not tracked at the queried revision
/// (US-031 criterion 3: "arquivo inexistente ... ou não versionado tem
/// resultado/erro definido"), or when the revision itself does not resolve
/// (US-032 criterion 3: "revisão inexistente ... não retorna dados de outra
/// versão"); Git reports these with "no such path", "bad revision" (an
/// unparsable expression) or "bad object" (a well-formed but nonexistent
/// hash) wording. Reclassifies as `ParseFailure` when the requested line range
/// falls outside the file (also US-032 criterion 3) — the same "reject
/// rather than silently fall back" guarantee `blame` already applies to a
/// structurally invalid range before the process even runs. Any other
/// failure passes through unchanged.
fn classify_blame_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("no such path")
        || diagnostic_text.contains("bad revision")
        || diagnostic_text.contains("bad object")
    {
        GitSailError::new(
            ErrorCode::RepositoryNotFound,
            "the file or revision could not be resolved",
        )
        .with_remediation("verify the file path and revision")
        .with_source(err)
    } else if diagnostic_text.contains("has only") {
        GitSailError::new(
            ErrorCode::ParseFailure,
            "the requested line range is out of bounds for the file",
        )
        .with_remediation("choose a line range within the file's length at the queried revision")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git log -L` as `RepositoryNotFound` when the
/// queried path does not exist at the queried revision or the revision
/// itself does not resolve (US-019 criterion 3), or as `ParseFailure` when
/// the requested line range falls outside the file — Git reports these with
/// "There is no path", "bad object"/"bad revision", and "has only N lines"
/// wording respectively, mirroring [`classify_blame_failure`]'s treatment
/// of the same underlying failure kinds for `git blame`. Any other failure
/// passes through unchanged.
fn classify_line_history_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("There is no path")
        || diagnostic_text.contains("bad revision")
        || diagnostic_text.contains("bad object")
        || diagnostic_text.contains("unknown revision or path not in the working tree")
    {
        GitSailError::new(
            ErrorCode::RepositoryNotFound,
            "the file or revision could not be resolved",
        )
        .with_remediation("verify the file path and revision")
        .with_source(err)
    } else if diagnostic_text.contains("has only") {
        GitSailError::new(
            ErrorCode::ParseFailure,
            "the requested line range is out of bounds for the file",
        )
        .with_remediation("choose a line range within the file's length at the queried revision")
        .with_source(err)
    } else {
        err
    }
}

/// Number of leading bytes inspected for an embedded NUL byte when deciding
/// whether `git show`'s output is binary content ([`classify_file_content`]).
const BINARY_SNIFF_LEN: usize = 8000;

/// Classifies raw `git show <rev>:<path>` stdout as [`FileContentKind::Text`]
/// or [`FileContentKind::Binary`] (EPIC-15/US-076): binary when a NUL byte
/// appears in the first [`BINARY_SNIFF_LEN`] bytes, or when the bytes are not
/// valid UTF-8 — mirroring the common heuristic Git itself uses to decide
/// whether a file is binary.
fn classify_file_content(stdout: &[u8]) -> FileContentKind {
    let sniff_len = stdout.len().min(BINARY_SNIFF_LEN);
    if stdout[..sniff_len].contains(&0) {
        return FileContentKind::Binary;
    }
    match String::from_utf8(stdout.to_vec()) {
        Ok(text) => FileContentKind::Text(text),
        Err(_) => FileContentKind::Binary,
    }
}

/// Reclassifies a failed `git branch -d/-D <name>` when it failed because
/// `name` is checked out (current branch or another worktree — US-023
/// criterion 2: "branch atual e branch em uso por worktree são protegidas")
/// or has unmerged commits and `-d` (not `-D`) was used (US-023 criterion 3:
/// "não é excluída à força implicitamente"). Any other failure passes
/// through unchanged.
fn classify_delete_branch_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("checked out at")
        || diagnostic_text.contains("which you are currently on")
        || diagnostic_text.contains("used by worktree")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "the branch is currently checked out",
        )
        .with_remediation(
            "switch to a different branch, or a different worktree, before deleting it",
        )
        .with_source(err)
    } else if diagnostic_text.contains("not fully merged") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "the branch has unmerged commits",
        )
        .with_remediation("merge the branch first, or delete it with force if you are sure")
        .with_source(err)
    } else {
        err
    }
}

/// Turns a failed `git stash apply`/`git stash pop` into a
/// [`StashApplyOutcome`] when the failure is actually a reported merge
/// conflict (US-093 criterion 2) — Git exits non-zero for this, but the
/// conflict text ("Auto-merging ...", "CONFLICT (content): Merge conflict
/// in ...") is on *stdout*, not stderr, verified directly against real Git
/// (`gitsail-git/src/runner.rs`'s `ProcessDiagnostic::stdout` field exists
/// specifically so this classification can see it). Any other
/// `ProcessFailure` is reclassified into a clearer error, or passed through
/// unchanged.
fn classify_stash_restore_outcome(err: GitSailError) -> Result<StashApplyOutcome, GitSailError> {
    if err.code() != ErrorCode::ProcessFailure {
        return Err(err);
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("CONFLICT") {
        Ok(StashApplyOutcome {
            had_conflicts: true,
        })
    } else if diagnostic_text.contains("would be overwritten") {
        Err(GitSailError::new(
            ErrorCode::OperationConflict,
            "restoring the stash would overwrite local changes",
        )
        .with_remediation("commit or stash your current local changes first, then retry")
        .with_source(err))
    } else if diagnostic_text.contains("No stash entries found")
        || diagnostic_text.contains("unknown option")
        || diagnostic_text.contains("Log for")
    {
        Err(classify_stash_missing_failure(err))
    } else {
        Err(err)
    }
}

/// Reclassifies a failed `git stash drop` (or a `git stash apply`/`pop`
/// whose failure [`classify_stash_restore_outcome`] determined was not a
/// conflict) as [`ErrorCode::OperationConflict`] when the named stash entry
/// does not exist. In ordinary use this is already prevented by
/// [`GitCliProvider::revalidate_stash_identity`] running first, but it is
/// kept here too as defense in depth against the small window between that
/// revalidation read and this mutation actually running. Any other failure
/// passes through unchanged.
fn classify_stash_missing_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("No stash entries found") || diagnostic_text.contains("Log for") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "the stash entry no longer exists",
        )
        .with_remediation("refresh the stash list and retry against a current entry")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git tag [-a -m ...] -- <name> [<target>]` when it
/// failed because `name` already exists (US-094 criterion 1: "colisão de
/// nome não sobrescreve silenciosamente") or `target` does not resolve to a
/// commit. Any other failure passes through unchanged.
fn classify_create_tag_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("already exists") {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "a tag with that name already exists",
        )
        .with_remediation("choose a different tag name")
        .with_source(err)
    } else if diagnostic_text.contains("not a valid object name") {
        GitSailError::new(
            ErrorCode::RepositoryNotFound,
            "the tag target does not exist",
        )
        .with_remediation("choose an existing commit, branch or tag as the target")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git tag -d -- <name>` when `name` does not exist.
/// Any other failure passes through unchanged.
fn classify_delete_tag_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("not found") {
        GitSailError::new(ErrorCode::RepositoryNotFound, "no such tag")
            .with_remediation("verify the tag name")
            .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git worktree add` when it failed because the
/// requested branch is already checked out in another worktree (US-095
/// criterion 2) or the target path already exists. Any other failure passes
/// through unchanged.
fn classify_create_worktree_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("already used by worktree") {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "that branch is already checked out in another worktree",
        )
        .with_remediation("choose a different branch, or remove/switch the other worktree first")
        .with_source(err)
    } else if diagnostic_text.contains("already exists") {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "the target path already exists",
        )
        .with_remediation("choose an empty or nonexistent path for the new worktree")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git worktree remove` when it failed because the
/// worktree has uncommitted changes and `force` was not set (US-095
/// criterion 3), is locked, or does not exist. Any other failure passes
/// through unchanged.
fn classify_remove_worktree_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("contains modified or untracked files") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "the worktree has uncommitted changes",
        )
        .with_remediation(
            "remove with force if you want to discard those changes, or commit/stash them first",
        )
        .with_source(err)
    } else if diagnostic_text.contains("cannot remove a locked working tree") {
        GitSailError::new(ErrorCode::OperationConflict, "the worktree is locked")
            .with_remediation("unlock it first (`git worktree unlock`), then retry")
            .with_source(err)
    } else if diagnostic_text.contains("is not a working tree")
        || diagnostic_text.contains("No such file or directory")
    {
        GitSailError::new(ErrorCode::RepositoryNotFound, "no such worktree")
            .with_remediation("verify the worktree path")
            .with_source(err)
    } else {
        err
    }
}

/// Reclassifies *any* failed Git invocation as [`ErrorCode::RepositoryLocked`]
/// when it failed because another Git process already holds `.git/
/// index.lock` (or another Git lock file) on this repository (SAD §26;
/// T-227/US-116 criterion 1: "uma mutação que falha porque outro processo
/// Git já segura o lock deve dar um erro claro, não travar nem corromper").
/// Applied centrally in [`GitCliProvider::run_cancellable`] rather than at
/// each individual mutation call site, so every command this adapter runs —
/// present or future — gets the same reclassification without needing its
/// own `map_err`. GitSail never waits for or removes another process's lock
/// file itself: doing so could corrupt state a still-live process is
/// writing. Any other failure passes through unchanged.
fn classify_index_lock_conflict(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains(".lock': File exists")
        || diagnostic_text.contains("Another git process seems to be running")
    {
        GitSailError::new(
            ErrorCode::RepositoryLocked,
            "another Git process is currently using this repository",
        )
        .with_remediation("wait for the other Git operation to finish, then retry")
        .with_source(err)
    } else {
        err
    }
}

// ---------------------------------------------------------------------
// Unified diff patch rendering for hunk-level stage/unstage (US-013).
// ---------------------------------------------------------------------

/// Renders `selection` into `git apply`-compatible unified diff text.
///
/// Thin wrapper over [`gitsail_application::render_unified_diff`]: the
/// actual rendering logic lives there (T-162/US-029) so this adapter's
/// hunk-level stage/unstage path and the patch-export use case TUI/Desktop
/// call for US-029 share exactly one renderer instead of two that could
/// drift apart.
fn render_hunk_patch(selection: &[FileDiff]) -> String {
    gitsail_application::render_unified_diff(selection)
}

fn parse_err(message: impl Into<String>) -> GitSailError {
    GitSailError::new(ErrorCode::ParseFailure, message.into())
}

/// Rejects `operation` up front, with an explicit error, when `repo` is
/// bare: a bare repository has no working tree, so `git` itself would
/// otherwise fail with an opaque, locale-dependent message (SAD §8, US-007
/// criterion 3: "bare repo ... operações dependentes de worktree retornam
/// limitação explícita").
fn require_worktree(repo: &Repository, operation: &str) -> Result<(), GitSailError> {
    if repo.is_bare {
        Err(GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            format!("{operation} requires a working tree, but this repository is bare"),
        )
        .with_remediation("open a non-bare repository, or a worktree of this bare repository"))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------
// `git status --porcelain=v2 --branch -z` parsing.
// ---------------------------------------------------------------------

fn parse_status(raw: &str) -> Result<RepositoryStatus, GitSailError> {
    let mut segments = raw.split('\0').filter(|s| !s.is_empty()).peekable();

    let mut oid_raw: Option<String> = None;
    let mut head_raw: Option<String> = None;
    while let Some(&segment) = segments.peek() {
        if let Some(rest) = segment.strip_prefix("# branch.oid ") {
            oid_raw = Some(rest.to_string());
            segments.next();
        } else if let Some(rest) = segment.strip_prefix("# branch.head ") {
            head_raw = Some(rest.to_string());
            segments.next();
        } else if segment.starts_with("# branch.") {
            // e.g. `branch.ab` (ahead/behind) or `branch.upstream`: not
            // part of RepositoryStatus, skip without interpreting.
            segments.next();
        } else {
            break;
        }
    }

    let head_state = resolve_status_head_state(oid_raw.as_deref(), head_raw.as_deref())?;
    let branch = match &head_state {
        HeadState::Attached { branch } => Some(branch.clone()),
        HeadState::Detached { .. } | HeadState::Unborn => None,
    };

    let mut files = Vec::new();
    while let Some(segment) = segments.next() {
        if let Some(change) = parse_status_entry(segment, &mut segments)? {
            files.push(change);
        }
    }

    Ok(RepositoryStatus {
        branch,
        head_state,
        files,
    })
}

fn resolve_status_head_state(
    oid_raw: Option<&str>,
    head_raw: Option<&str>,
) -> Result<HeadState, GitSailError> {
    match (oid_raw, head_raw) {
        (Some("(initial)"), _) => Ok(HeadState::Unborn),
        (Some(oid), Some("(detached)")) => {
            let commit = CommitHash::new(oid.to_string())?;
            Ok(HeadState::Detached { commit })
        }
        (Some(_), Some(head)) => {
            let branch = BranchName::new(head.to_string())?;
            Ok(HeadState::Attached { branch })
        }
        _ => Err(parse_err(
            "git status output missing branch.oid/branch.head headers",
        )),
    }
}

/// Parses one non-header `-z` status segment, consuming an extra segment
/// from `rest` for rename/copy entries (their `origPath` is its own
/// NUL-terminated segment, not part of this one).
fn parse_status_entry<'a, I: Iterator<Item = &'a str>>(
    segment: &'a str,
    rest: &mut I,
) -> Result<Option<FileChange>, GitSailError> {
    match segment.as_bytes().first() {
        Some(b'1') => parse_ordinary_status_entry(segment).map(Some),
        Some(b'2') => parse_rename_status_entry(segment, rest).map(Some),
        Some(b'u') => parse_unmerged_status_entry(segment).map(Some),
        Some(b'?') => {
            parse_marker_status_entry(segment, FileStatusCode::Untracked, ChangeType::Untracked)
                .map(Some)
        }
        Some(b'!') => {
            parse_marker_status_entry(segment, FileStatusCode::Ignored, ChangeType::Ignored)
                .map(Some)
        }
        _ => Err(parse_err("unrecognized git status entry kind")),
    }
}

fn split_status_fields(segment: &str, count: usize) -> Result<Vec<&str>, GitSailError> {
    let fields: Vec<&str> = segment.splitn(count, ' ').collect();
    if fields.len() != count {
        return Err(parse_err(
            "malformed git status entry: unexpected field count",
        ));
    }
    Ok(fields)
}

fn status_xy(xy: &str) -> Result<(char, char), GitSailError> {
    let mut chars = xy.chars();
    let x = chars
        .next()
        .ok_or_else(|| parse_err("missing XY status code"))?;
    let y = chars
        .next()
        .ok_or_else(|| parse_err("missing XY status code"))?;
    if chars.next().is_some() {
        return Err(parse_err("XY status code must be exactly two characters"));
    }
    Ok((x, y))
}

fn status_code(code: char) -> Result<FileStatusCode, GitSailError> {
    match code {
        '.' => Ok(FileStatusCode::Unmodified),
        'M' => Ok(FileStatusCode::Modified),
        'A' => Ok(FileStatusCode::Added),
        'D' => Ok(FileStatusCode::Deleted),
        'R' => Ok(FileStatusCode::Renamed),
        'C' => Ok(FileStatusCode::Copied),
        'U' => Ok(FileStatusCode::UpdatedButUnmerged),
        // A type change (regular file <-> symlink <-> submodule) has no
        // dedicated `FileStatusCode` variant; it is reported as Modified
        // at the per-side status level, while `ChangeType::TypeChanged`
        // (see `change_type_for_code`) preserves the distinction.
        'T' => Ok(FileStatusCode::Modified),
        other => Err(parse_err(format!(
            "unrecognized git status XY code '{other}'"
        ))),
    }
}

fn change_type_for_code(code: char) -> Result<ChangeType, GitSailError> {
    match code {
        'A' => Ok(ChangeType::Added),
        'D' => Ok(ChangeType::Deleted),
        'M' => Ok(ChangeType::Modified),
        'T' => Ok(ChangeType::TypeChanged),
        'R' => Ok(ChangeType::Renamed),
        'C' => Ok(ChangeType::Copied),
        'U' => Ok(ChangeType::Unmerged),
        other => Err(parse_err(format!(
            "unrecognized git status change code '{other}'"
        ))),
    }
}

fn parse_ordinary_status_entry(segment: &str) -> Result<FileChange, GitSailError> {
    // `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`
    let fields = split_status_fields(segment, 9)?;
    let (x, y) = status_xy(fields[1])?;
    let index_status = status_code(x)?;
    let worktree_status = status_code(y)?;
    let representative = if x != '.' { x } else { y };
    let change_type = change_type_for_code(representative)?;
    Ok(FileChange {
        path: PathBuf::from(fields[8]),
        previous_path: None,
        change_type,
        index_status,
        worktree_status,
    })
}

fn parse_rename_status_entry<'a, I: Iterator<Item = &'a str>>(
    segment: &'a str,
    rest: &mut I,
) -> Result<FileChange, GitSailError> {
    // `2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <Xscore> <path>`, followed by
    // a separate `origPath` segment.
    let fields = split_status_fields(segment, 10)?;
    let (x, y) = status_xy(fields[1])?;
    let index_status = status_code(x)?;
    let worktree_status = status_code(y)?;
    let orig_path = rest
        .next()
        .ok_or_else(|| parse_err("rename/copy status entry missing origPath segment"))?;
    let change_type = match x {
        'R' => ChangeType::Renamed,
        'C' => ChangeType::Copied,
        other => change_type_for_code(other)?,
    };
    Ok(FileChange {
        path: PathBuf::from(fields[9]),
        previous_path: Some(PathBuf::from(orig_path)),
        change_type,
        index_status,
        worktree_status,
    })
}

fn parse_unmerged_status_entry(segment: &str) -> Result<FileChange, GitSailError> {
    // `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`
    let fields = split_status_fields(segment, 11)?;
    let (x, y) = status_xy(fields[1])?;
    let index_status = status_code(x)?;
    let worktree_status = status_code(y)?;
    Ok(FileChange {
        path: PathBuf::from(fields[10]),
        previous_path: None,
        change_type: ChangeType::Unmerged,
        index_status,
        worktree_status,
    })
}

fn parse_marker_status_entry(
    segment: &str,
    status: FileStatusCode,
    change_type: ChangeType,
) -> Result<FileChange, GitSailError> {
    // `? <path>` (untracked) or `! <path>` (ignored).
    let fields = split_status_fields(segment, 2)?;
    Ok(FileChange {
        path: PathBuf::from(fields[1]),
        previous_path: None,
        change_type,
        index_status: status,
        worktree_status: status,
    })
}

// ---------------------------------------------------------------------
// `git log` parsing (shared by `commits` and `commit`).
// ---------------------------------------------------------------------

fn parse_log_records(raw: &str) -> Result<Vec<Commit>, GitSailError> {
    raw.split(RECORD_SEP)
        // `git log --pretty=format:...` inserts a newline after every
        // record but the last; strip the leading one each record but the
        // first would otherwise carry.
        .map(|record| record.strip_prefix('\n').unwrap_or(record))
        .filter(|record| !record.is_empty())
        .map(parse_log_record)
        .collect()
}

fn parse_log_record(record: &str) -> Result<Commit, GitSailError> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != LOG_FIELD_COUNT {
        return Err(parse_err(
            "malformed git log record: unexpected field count",
        ));
    }

    let hash = CommitHash::new(fields[0].to_string())?;
    let short_hash = ShortHash::new(fields[1].to_string())?;
    let parents = fields[2]
        .split(' ')
        .filter(|s| !s.is_empty())
        .map(|s| CommitHash::new(s.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let author = Signature::new(fields[3].to_string(), fields[4].to_string());
    let committer = Signature::new(fields[5].to_string(), fields[6].to_string());
    let author_date = parse_raw_date(fields[7])?;
    let commit_date = parse_raw_date(fields[8])?;
    let subject = fields[9].to_string();
    let body = fields[10].to_string();
    let decorations = parse_decorations(fields[11])?;

    Ok(Commit {
        hash,
        short_hash,
        parents,
        author,
        committer,
        author_date,
        commit_date,
        subject,
        body,
        decorations,
    })
}

/// Parses a `--date=raw` timestamp: `"<epoch seconds> <+HHMM|-HHMM>"`.
fn parse_raw_date(field: &str) -> Result<GitTimestamp, GitSailError> {
    let mut parts = field.split(' ');
    let seconds_str = parts
        .next()
        .ok_or_else(|| parse_err("missing timestamp seconds"))?;
    let offset_str = parts
        .next()
        .ok_or_else(|| parse_err("missing timestamp offset"))?;
    if parts.next().is_some() {
        return Err(parse_err("malformed --date=raw timestamp"));
    }
    let seconds_since_epoch: i64 = seconds_str
        .parse()
        .map_err(|_| parse_err("timestamp seconds were not an integer"))?;
    let utc_offset_minutes = parse_utc_offset(offset_str)?;
    Ok(GitTimestamp::new(seconds_since_epoch, utc_offset_minutes))
}

/// Parses a `+HHMM`/`-HHMM` UTC offset into signed minutes.
fn parse_utc_offset(offset: &str) -> Result<i32, GitSailError> {
    if offset.len() != 5 {
        return Err(parse_err("timestamp offset must be s HHMM"));
    }
    let sign = match &offset[0..1] {
        "+" => 1,
        "-" => -1,
        _ => return Err(parse_err("timestamp offset must start with + or -")),
    };
    let hours: i32 = offset[1..3]
        .parse()
        .map_err(|_| parse_err("timestamp offset hours were not numeric"))?;
    let minutes: i32 = offset[3..5]
        .parse()
        .map_err(|_| parse_err("timestamp offset minutes were not numeric"))?;
    Ok(sign * (hours * 60 + minutes))
}

/// Parses a `%D` decoration string (e.g. `HEAD -> main, origin/main, tag:
/// v1.0`). Only the `origin/` remote prefix is special-cased per the
/// adapter's design (matching the suggested parsing rules): decorations for
/// any other remote fall back to a plain `Decoration::Branch`, which is a
/// known, documented simplification.
fn parse_decorations(field: &str) -> Result<Vec<Decoration>, GitSailError> {
    if field.is_empty() {
        return Ok(Vec::new());
    }
    let mut decorations = Vec::new();
    for part in field.split(", ") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(branch) = part.strip_prefix("HEAD -> ") {
            decorations.push(Decoration::Head);
            decorations.push(Decoration::Branch(BranchName::new(branch.to_string())?));
        } else if part == "HEAD" {
            decorations.push(Decoration::Head);
        } else if let Some(tag) = part.strip_prefix("tag: ") {
            decorations.push(Decoration::Tag(tag.to_string()));
        } else if let Some(branch) = part.strip_prefix("origin/") {
            decorations.push(Decoration::RemoteBranch {
                remote: "origin".to_string(),
                branch: BranchName::new(branch.to_string())?,
            });
        } else {
            decorations.push(Decoration::Branch(BranchName::new(part.to_string())?));
        }
    }
    Ok(decorations)
}

// ---------------------------------------------------------------------
// `git for-each-ref` parsing.
// ---------------------------------------------------------------------

fn parse_ref_line(line: &str) -> Result<Option<Branch>, GitSailError> {
    let fields: Vec<&str> = line.split(FIELD_SEP).collect();
    if fields.len() != 5 {
        return Err(parse_err(
            "malformed for-each-ref record: unexpected field count",
        ));
    }
    let refname = fields[0];
    let objectname = fields[1];
    let upstream_short = fields[2];
    let upstream_track = fields[3];
    let head_marker = fields[4];

    let (kind, name) = if let Some(rest) = refname.strip_prefix("refs/heads/") {
        (BranchKind::Local, rest.to_string())
    } else if let Some(rest) = refname.strip_prefix("refs/remotes/") {
        let mut parts = rest.splitn(2, '/');
        let remote = parts
            .next()
            .ok_or_else(|| parse_err("malformed remote ref name: missing remote"))?;
        let branch = parts
            .next()
            .ok_or_else(|| parse_err("malformed remote ref name: missing branch"))?;
        if branch == "HEAD" {
            // `refs/remotes/<remote>/HEAD` is a symbolic ref to the
            // remote's default branch, not a branch in its own right.
            return Ok(None);
        }
        (
            BranchKind::Remote {
                remote: remote.to_string(),
            },
            branch.to_string(),
        )
    } else {
        return Err(parse_err(
            "unrecognized ref namespace in for-each-ref output",
        ));
    };

    let name = BranchName::new(name)?;
    let target = CommitHash::new(objectname.to_string())?;
    let upstream = if upstream_short.is_empty() {
        None
    } else {
        Some(BranchName::new(upstream_short.to_string())?)
    };
    let (ahead, behind) = parse_ahead_behind(upstream_track)?;
    let is_current = head_marker == "*";

    Ok(Some(Branch {
        name,
        kind,
        target,
        upstream,
        ahead,
        behind,
        is_current,
    }))
}

/// Parses `%(upstream:track)`: empty, `[gone]`, `[ahead N]`, `[behind M]`
/// or `[ahead N, behind M]`.
fn parse_ahead_behind(track: &str) -> Result<(u32, u32), GitSailError> {
    let track = track.trim();
    if track.is_empty() || track == "[gone]" {
        return Ok((0, 0));
    }
    let inner = track
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| parse_err("malformed upstream:track value"))?;

    let mut ahead = 0u32;
    let mut behind = 0u32;
    for part in inner.split(", ") {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n
                .parse()
                .map_err(|_| parse_err("malformed ahead count in upstream:track"))?;
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n
                .parse()
                .map_err(|_| parse_err("malformed behind count in upstream:track"))?;
        } else {
            return Err(parse_err("unrecognized upstream:track segment"));
        }
    }
    Ok((ahead, behind))
}

// ---------------------------------------------------------------------
// `git for-each-ref refs/tags` parsing (EPIC-18/T-216/US-091).
// ---------------------------------------------------------------------

/// Splits `raw` `for-each-ref`/`stash list` `RECORD_SEP`-terminated output
/// into individual records, mirroring [`parse_log_records`]'s exact
/// convention (Git inserts a `\n` before every record but the first, which
/// is stripped here the same way).
fn split_record_sep_blocks(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(RECORD_SEP)
        .map(|record| record.strip_prefix('\n').unwrap_or(record))
        .filter(|record| !record.is_empty())
}

fn parse_tag_records(raw: &str) -> Result<Vec<Tag>, GitSailError> {
    split_record_sep_blocks(raw).map(parse_tag_record).collect()
}

fn parse_tag_record(record: &str) -> Result<Tag, GitSailError> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != TAG_FIELD_COUNT {
        return Err(parse_err(
            "malformed for-each-ref tag record: unexpected field count",
        ));
    }
    let name = fields[0].to_string();
    let object_name = fields[1];
    let object_type = fields[2];
    let peeled = fields[3];
    let tagger_name = fields[4];
    let tagger_email = fields[5];
    let tagger_date = fields[6];
    let contents = fields[7];

    match object_type {
        "tag" => {
            // An annotated tag: `%(objectname)` is the tag object itself,
            // never a commit — the commit this tag names is
            // `%(*objectname)` (the peeled/dereferenced target).
            if peeled.is_empty() {
                return Err(parse_err(
                    "annotated tag record missing its peeled (dereferenced) target",
                ));
            }
            let target = CommitHash::new(peeled.to_string())?;
            if tagger_name.is_empty() {
                return Err(parse_err("annotated tag record missing tagger name"));
            }
            let tagger = Signature::new(tagger_name.to_string(), strip_angle_brackets(tagger_email));
            let date = parse_raw_date(tagger_date)?;
            // `%(contents)` always reports a trailing newline for a real
            // message; trimmed once here so `Tag::kind`'s message matches
            // what a person actually typed, not Git's own storage
            // convention.
            let message = contents.strip_suffix('\n').unwrap_or(contents).to_string();
            Ok(Tag {
                name,
                target,
                kind: TagKind::Annotated {
                    message,
                    tagger,
                    date,
                },
            })
        }
        "commit" => {
            // A lightweight tag pointing directly at a commit.
            let target = CommitHash::new(object_name.to_string())?;
            Ok(Tag {
                name,
                target,
                kind: TagKind::Lightweight,
            })
        }
        other => Err(parse_err(format!(
            "tag '{name}' points at an unsupported object type '{other}' (expected a commit or an annotated tag)"
        ))),
    }
}

// ---------------------------------------------------------------------
// `git stash list` parsing (EPIC-18/T-216/US-091).
// ---------------------------------------------------------------------

fn parse_stash_records(raw: &str) -> Result<Vec<Stash>, GitSailError> {
    split_record_sep_blocks(raw)
        .enumerate()
        .map(|(index, record)| parse_stash_record(index as u32, record))
        .collect()
}

fn parse_stash_record(index: u32, record: &str) -> Result<Stash, GitSailError> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != STASH_FIELD_COUNT {
        return Err(parse_err(
            "malformed git stash list record: unexpected field count",
        ));
    }
    let commit = CommitHash::new(fields[0].to_string())?;
    let message = fields[1].to_string();
    let date = parse_raw_date(fields[2])?;
    Ok(Stash {
        index,
        commit,
        message,
        date,
    })
}

// ---------------------------------------------------------------------
// `git worktree list --porcelain -z` parsing (EPIC-18/T-220/US-095).
// ---------------------------------------------------------------------

/// Parses `raw` `git worktree list --porcelain -z` output: each worktree's
/// block of NUL-separated lines is itself terminated by an empty field,
/// i.e. two consecutive NULs — [`str::split`] on that exact two-byte
/// separator reproduces this reliably (unlike splitting on a single NUL and
/// filtering empty lines, which cannot distinguish a genuine empty line
/// from the block boundary).
fn parse_worktree_records(raw: &str) -> Result<Vec<Worktree>, GitSailError> {
    raw.split("\0\0")
        .filter(|block| !block.is_empty())
        .enumerate()
        .map(|(index, block)| parse_worktree_record(index == 0, block))
        .collect()
}

fn parse_worktree_record(is_main: bool, block: &str) -> Result<Worktree, GitSailError> {
    let mut path: Option<PathBuf> = None;
    let mut head_hash: Option<&str> = None;
    let mut branch_ref: Option<&str> = None;
    let mut detached = false;
    let mut is_locked = false;
    let mut is_prunable = false;

    for line in block.split('\0').filter(|l| !l.is_empty()) {
        if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            head_hash = Some(rest);
        } else if let Some(rest) = line.strip_prefix("branch ") {
            branch_ref = Some(rest);
        } else if line == "detached" {
            detached = true;
        } else if line == "locked" || line.starts_with("locked ") {
            is_locked = true;
        } else if line == "prunable" || line.starts_with("prunable ") {
            is_prunable = true;
        }
        // Any other line (e.g. "bare", for a bare repository's own
        // worktree entry) is not modeled by `Worktree` and is ignored
        // rather than rejected — a forward-compatible parse, matching how
        // `parse_decorations` already treats an unrecognized ref namespace.
    }

    let path = path.ok_or_else(|| parse_err("worktree record missing a path"))?;
    let head = match head_hash {
        None => return Err(parse_err("worktree record missing HEAD")),
        Some(hash) => {
            let commit = CommitHash::new(hash.to_string())?;
            if commit.is_zero() {
                WorktreeHead::Unborn
            } else if detached {
                WorktreeHead::Detached { commit }
            } else if let Some(refname) = branch_ref {
                let branch_name = refname.strip_prefix("refs/heads/").unwrap_or(refname);
                WorktreeHead::Attached {
                    branch: BranchName::new(branch_name.to_string())?,
                }
            } else {
                return Err(parse_err(
                    "worktree record has neither a branch nor a detached marker",
                ));
            }
        }
    };

    Ok(Worktree {
        path,
        head,
        is_main,
        is_locked,
        is_prunable,
    })
}

// ---------------------------------------------------------------------
// `git diff` unified patch parsing.
// ---------------------------------------------------------------------

/// Per-file cap on parsed hunk content, independent of the process-wide
/// stream cap (`MAX_CAPTURED_STREAM_BYTES` in `runner.rs`): a single huge
/// file should not be fully materialized into memory just because the
/// overall diff output was small enough to be captured (US-027 criterion
/// 3). Hunks beyond this cap are withheld and `FileDiff::truncated` is set
/// instead, rather than parsing partial/misleading hunk data.
const MAX_FILE_DIFF_BYTES: usize = 512 * 1024;

/// Progress-check stride for cooperative cancellation during parsing
/// (T-226/US-115 criterion 2): the process-level check in
/// `GitProcessRunner` only covers the Git subprocess's own lifetime — once
/// it has exited, parsing its captured output is pure Rust-side work with
/// no process to poll. A very large diff (many files, or one file with many
/// hunks) or blame result can make that parsing itself the slow part, so
/// [`parse_diff`]/[`parse_diff_block`]/[`parse_blame`] all check
/// periodically during their own loops, not only once at the start.
const CANCEL_CHECK_STRIDE: usize = 256;

fn parse_diff(raw: &str, cancel: &CancellationToken) -> Result<Diff, GitSailError> {
    let mut files = Vec::new();
    for block in split_diff_blocks(raw) {
        if cancel.is_cancelled() {
            return Err(cancelled_during_parse());
        }
        files.push(parse_diff_block(block, cancel)?);
    }
    Ok(Diff { files })
}

/// A parse loop's own cancellation error (T-226/US-115 criterion 2/3):
/// categorically an `Err`, exactly like a subprocess-level cancellation
/// (`ErrorCode::Cancelled`), never a partial `Ok` — a cancelled parse must
/// never be mistaken for "complete, no changes" (criterion 3).
fn cancelled_during_parse() -> GitSailError {
    GitSailError::new(
        ErrorCode::Cancelled,
        "operation was cancelled while parsing the result",
    )
}

/// Splits `raw` on `\n` without stripping a preceding `\r`, unlike
/// [`str::lines`] — a CRLF file's content lines keep their `\r` so it
/// round-trips through [`DiffLine::content`] (US-027 criterion 2). A single
/// trailing empty element from a final `\n` is dropped to match
/// `str::lines`'s behavior for the last line.
fn raw_lines(raw: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = raw.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

/// Splits a unified patch into per-file blocks, each starting with its
/// `diff --git a/... b/...` header line. Any bytes before the first such
/// header (not expected in practice) are discarded rather than misread.
fn split_diff_blocks(raw: &str) -> Vec<Vec<&str>> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in raw_lines(raw) {
        if line.starts_with("diff --git ") {
            blocks.push(vec![line]);
        } else if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
    }
    blocks
}

fn parse_diff_block(
    lines: Vec<&str>,
    cancel: &CancellationToken,
) -> Result<FileDiff, GitSailError> {
    let header = *lines.first().ok_or_else(|| parse_err("empty diff block"))?;

    let mut previous_path: Option<PathBuf> = None;
    let mut is_copy = false;
    let mut is_new_file = false;
    let mut is_deleted_file = false;
    let mut is_binary = false;
    // `Some(None)` means the marker line was seen and pointed at
    // `/dev/null`; `None` means the marker line was never seen at all
    // (e.g. a pure binary or pure mode-change diff).
    let mut marker_old_path: Option<Option<PathBuf>> = None;
    let mut marker_new_path: Option<Option<PathBuf>> = None;
    let mut binary_old_path: Option<PathBuf> = None;
    let mut binary_new_path: Option<PathBuf> = None;
    let mut hunks = Vec::new();
    let mut hunk_bytes_total = 0usize;
    let mut truncated = false;

    let mut i = 1;
    while i < lines.len() {
        if i % CANCEL_CHECK_STRIDE == 0 && cancel.is_cancelled() {
            return Err(cancelled_during_parse());
        }
        let line = lines[i];
        if let Some(rest) = line.strip_prefix("rename from ") {
            previous_path = Some(PathBuf::from(rest));
            i += 1;
        } else if line.starts_with("rename to ") {
            i += 1;
        } else if let Some(rest) = line.strip_prefix("copy from ") {
            previous_path = Some(PathBuf::from(rest));
            is_copy = true;
            i += 1;
        } else if line.starts_with("copy to ") {
            i += 1;
        } else if line.starts_with("new file mode") {
            is_new_file = true;
            i += 1;
        } else if line.starts_with("deleted file mode") {
            is_deleted_file = true;
            i += 1;
        } else if let Some(rest) = line.strip_prefix("Binary files ") {
            if let Some(inner) = rest.strip_suffix(" differ") {
                is_binary = true;
                // Best-effort split on the first " and "; a path literally
                // containing that substring is a known, documented
                // limitation of this fallback.
                if let Some(sep) = inner.find(" and ") {
                    binary_old_path = strip_diff_path_prefix(&inner[..sep], "a/");
                    binary_new_path = strip_diff_path_prefix(&inner[sep + " and ".len()..], "b/");
                }
            }
            i += 1;
        } else if let Some(rest) = line.strip_prefix("--- ") {
            marker_old_path = Some(strip_diff_path_prefix(rest, "a/"));
            i += 1;
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            marker_new_path = Some(strip_diff_path_prefix(rest, "b/"));
            i += 1;
        } else if line.starts_with("@@ ") {
            let (hunk, consumed) = parse_diff_hunk(&lines, i)?;
            if !truncated {
                let hunk_bytes: usize = hunk.lines.iter().map(|l| l.content.len() + 1).sum();
                if hunk_bytes_total + hunk_bytes > MAX_FILE_DIFF_BYTES {
                    truncated = true;
                } else {
                    hunk_bytes_total += hunk_bytes;
                    hunks.push(hunk);
                }
            }
            i += consumed;
        } else {
            i += 1;
        }
    }

    let (header_old, header_new) = parse_diff_git_header(header)?;
    let old_path = marker_old_path
        .clone()
        .flatten()
        .or(binary_old_path)
        .unwrap_or(header_old);
    let new_path = marker_new_path
        .clone()
        .flatten()
        .or(binary_new_path)
        .unwrap_or(header_new);
    let old_is_dev_null = matches!(marker_old_path, Some(None));
    let new_is_dev_null = matches!(marker_new_path, Some(None));

    let (change_type, path, previous_path) = if previous_path.is_some() {
        let kind = if is_copy {
            ChangeType::Copied
        } else {
            ChangeType::Renamed
        };
        (kind, new_path, previous_path)
    } else if is_new_file || old_is_dev_null {
        (ChangeType::Added, new_path, None)
    } else if is_deleted_file || new_is_dev_null {
        (ChangeType::Deleted, old_path, None)
    } else {
        (ChangeType::Modified, new_path, None)
    };

    Ok(FileDiff {
        path,
        previous_path,
        change_type,
        is_binary,
        truncated,
        hunks,
    })
}

/// Strips `prefix` (`a/`/`b/`) from a `---`/`+++` marker path. Git appends a
/// trailing tab to disambiguate the filename from the (omitted) legacy
/// unified-diff timestamp whenever the name itself contains whitespace
/// (US-008: e.g. a Unicode filename with a space) — stripped here so it
/// never leaks into the parsed path.
fn strip_diff_path_prefix(raw: &str, prefix: &str) -> Option<PathBuf> {
    let raw = raw.strip_suffix('\t').unwrap_or(raw);
    if raw == "/dev/null" {
        None
    } else if let Some(stripped) = raw.strip_prefix(prefix) {
        Some(PathBuf::from(stripped))
    } else {
        Some(PathBuf::from(raw))
    }
}

/// Fallback path extraction from the `diff --git a/<old> b/<new>` header
/// line, used only when neither `---`/`+++` markers nor a `Binary files`
/// line supplied paths (e.g. a pure file-mode change). Splits on the last
/// `" b/"`; a path containing that exact substring is a known, documented
/// limitation of this fallback (SAD §31 accepts imperfect coverage of
/// exotic patch formats).
fn parse_diff_git_header(line: &str) -> Result<(PathBuf, PathBuf), GitSailError> {
    let rest = line
        .strip_prefix("diff --git ")
        .ok_or_else(|| parse_err("diff block missing 'diff --git' header"))?;
    let rest = rest
        .strip_prefix("a/")
        .ok_or_else(|| parse_err("diff --git header missing 'a/' prefix"))?;
    let split_at = rest
        .rfind(" b/")
        .ok_or_else(|| parse_err("diff --git header missing 'b/' prefix"))?;
    let old = &rest[..split_at];
    let new = &rest[split_at + " b/".len()..];
    Ok((PathBuf::from(old), PathBuf::from(new)))
}

fn parse_diff_hunk(lines: &[&str], start: usize) -> Result<(DiffHunk, usize), GitSailError> {
    let (old_start, old_lines, new_start, new_lines) = parse_diff_hunk_header(lines[start])?;

    let mut content_lines: Vec<DiffLine> = Vec::new();
    let mut i = start + 1;
    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("@@ ") || line.starts_with("diff --git ") {
            break;
        }
        let origin = match line.as_bytes().first() {
            Some(b' ') => DiffLineOrigin::Context,
            Some(b'+') => DiffLineOrigin::Addition,
            Some(b'-') => DiffLineOrigin::Deletion,
            // "\ No newline at end of file" refers to the content line
            // immediately preceding it (US-027 criterion 2); any other
            // stray marker line is skipped without ending the hunk.
            _ => {
                if line == "\\ No newline at end of file" {
                    if let Some(last) = content_lines.last_mut() {
                        last.has_trailing_newline = false;
                    }
                }
                i += 1;
                continue;
            }
        };
        content_lines.push(DiffLine {
            origin,
            content: line[1..].to_string(),
            has_trailing_newline: true,
        });
        i += 1;
    }

    let hunk = DiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        lines: content_lines,
    };
    Ok((hunk, i - start))
}

/// Parses `@@ -old_start[,old_lines] +new_start[,new_lines] @@[ context]`.
/// A missing `,count` means a single-line range (defaults to 1).
fn parse_diff_hunk_header(line: &str) -> Result<(u32, u32, u32, u32), GitSailError> {
    let rest = line
        .strip_prefix("@@ ")
        .ok_or_else(|| parse_err("malformed diff hunk header"))?;
    let end = rest
        .find(" @@")
        .ok_or_else(|| parse_err("malformed diff hunk header: missing closing '@@'"))?;
    let ranges = &rest[..end];
    let mut parts = ranges.split(' ');
    let old_range = parts
        .next()
        .ok_or_else(|| parse_err("diff hunk header missing old range"))?;
    let new_range = parts
        .next()
        .ok_or_else(|| parse_err("diff hunk header missing new range"))?;
    let (old_start, old_lines) = parse_diff_hunk_range(old_range, '-')?;
    let (new_start, new_lines) = parse_diff_hunk_range(new_range, '+')?;
    Ok((old_start, old_lines, new_start, new_lines))
}

fn parse_diff_hunk_range(range: &str, sigil: char) -> Result<(u32, u32), GitSailError> {
    let range = range
        .strip_prefix(sigil)
        .ok_or_else(|| parse_err("diff hunk range missing sigil"))?;
    let mut parts = range.splitn(2, ',');
    let start: u32 = parts
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| parse_err("diff hunk range missing start"))?
        .parse()
        .map_err(|_| parse_err("diff hunk range start was not numeric"))?;
    let count: u32 = match parts.next() {
        Some(c) => c
            .parse()
            .map_err(|_| parse_err("diff hunk range count was not numeric"))?,
        None => 1,
    };
    Ok((start, count))
}

// ---------------------------------------------------------------------
// `git blame --porcelain` parsing.
// ---------------------------------------------------------------------

fn parse_blame(raw: &str, cancel: &CancellationToken) -> Result<Vec<BlameLine>, GitSailError> {
    let mut metadata: HashMap<String, (Signature, GitTimestamp)> = HashMap::new();
    let mut lines_out = Vec::new();

    let mut current_hash: Option<String> = None;
    let mut current_final_line = 0u32;
    let mut current_original_line = 0u32;

    let mut pending_author_name: Option<String> = None;
    let mut pending_author_email: Option<String> = None;
    let mut pending_author_time: Option<i64> = None;
    let mut pending_author_tz: Option<i32> = None;

    for (index, line) in raw.split('\n').enumerate() {
        if index % CANCEL_CHECK_STRIDE == 0 && cancel.is_cancelled() {
            return Err(cancelled_during_parse());
        }
        if line.is_empty() {
            continue;
        }
        if let Some(content) = line.strip_prefix('\t') {
            let hash = current_hash
                .clone()
                .ok_or_else(|| parse_err("blame content line without a preceding header"))?;
            let (author, timestamp) = metadata.get(&hash).cloned().ok_or_else(|| {
                parse_err("blame content line references unknown commit metadata")
            })?;
            let commit = CommitHash::new(hash)?;
            let origin = if commit.is_zero() {
                BlameOrigin::Local
            } else {
                BlameOrigin::Committed
            };
            lines_out.push(BlameLine {
                final_line: current_final_line,
                original_line: current_original_line,
                commit,
                author,
                timestamp,
                content: content.to_string(),
                origin,
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("author ") {
            pending_author_name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("author-mail ") {
            pending_author_email = Some(strip_angle_brackets(rest));
        } else if let Some(rest) = line.strip_prefix("author-time ") {
            pending_author_time = Some(
                rest.parse()
                    .map_err(|_| parse_err("blame author-time was not numeric"))?,
            );
        } else if let Some(rest) = line.strip_prefix("author-tz ") {
            pending_author_tz = Some(parse_utc_offset(rest)?);
        } else if is_blame_header_line(line) {
            let (hash, orig_line, final_line) = parse_blame_header(line)?;
            let is_new_commit = !metadata.contains_key(&hash);
            current_hash = Some(hash);
            current_original_line = orig_line;
            current_final_line = final_line;
            if is_new_commit {
                pending_author_name = None;
                pending_author_email = None;
                pending_author_time = None;
                pending_author_tz = None;
            }
        }
        // Other metadata lines (`committer*`, `summary`, `previous`,
        // `filename`, `boundary`) are not needed by the `Blame` domain
        // type and are ignored without erroring.

        if let (Some(hash), Some(name), Some(email), Some(time), Some(tz)) = (
            current_hash.clone(),
            pending_author_name.clone(),
            pending_author_email.clone(),
            pending_author_time,
            pending_author_tz,
        ) {
            metadata
                .entry(hash)
                .or_insert_with(|| (Signature::new(name, email), GitTimestamp::new(time, tz)));
        }
    }

    Ok(lines_out)
}

fn strip_angle_brackets(raw: &str) -> String {
    raw.trim_start_matches('<')
        .trim_end_matches('>')
        .to_string()
}

/// A blame header line is `<hash> <orig_line> <final_line> [<count>]`;
/// distinguishes it from metadata lines (`author ...`, `summary ...`, ...)
/// by requiring the first token to be all hex digits and the next two to
/// be integers.
fn is_blame_header_line(line: &str) -> bool {
    let mut parts = line.split(' ');
    let Some(first) = parts.next() else {
        return false;
    };
    if first.is_empty() || !first.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    let Some(second) = parts.next() else {
        return false;
    };
    let Some(third) = parts.next() else {
        return false;
    };
    second.parse::<u32>().is_ok() && third.parse::<u32>().is_ok()
}

fn parse_blame_header(line: &str) -> Result<(String, u32, u32), GitSailError> {
    let mut parts = line.split(' ');
    let hash = parts
        .next()
        .ok_or_else(|| parse_err("missing blame commit hash"))?
        .to_string();
    let original_line: u32 = parts
        .next()
        .ok_or_else(|| parse_err("missing blame original line number"))?
        .parse()
        .map_err(|_| parse_err("blame original line number was not numeric"))?;
    let final_line: u32 = parts
        .next()
        .ok_or_else(|| parse_err("missing blame final line number"))?
        .parse()
        .map_err(|_| parse_err("blame final line number was not numeric"))?;
    Ok((hash, original_line, final_line))
}

// ---------------------------------------------------------------------
// `git log -L ... --pretty=format:%H%x1e` line-history parsing.
// ---------------------------------------------------------------------

/// A header line is exactly the 40-character commit hash immediately
/// followed by `RECORD_SEP` and nothing else — unambiguous, since a diff
/// line can never start with 40 hex characters followed immediately by
/// that control character.
fn parse_line_history_header(line: &str) -> Option<&str> {
    const HASH_LEN: usize = 40;
    if line.len() != HASH_LEN + RECORD_SEP.len_utf8() {
        return None;
    }
    let (hash, rest) = line.split_at(HASH_LEN);
    if !rest.starts_with(RECORD_SEP) {
        return None;
    }
    hash.bytes().all(|b| b.is_ascii_hexdigit()).then_some(hash)
}

/// Splits `-L`-with-hash-header output into one `(hash, diff_lines)` block
/// per commit, most-recent-first (US-019). A line before the first header
/// (not expected in practice) is discarded rather than misread as diff
/// content belonging to no commit.
fn split_line_history_blocks(raw: &str) -> Result<Vec<(String, Vec<&str>)>, GitSailError> {
    let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();
    for line in raw_lines(raw) {
        if let Some(hash) = parse_line_history_header(line) {
            blocks.push((hash.to_string(), Vec::new()));
        } else if let Some((_, diff_lines)) = blocks.last_mut() {
            diff_lines.push(line);
        }
    }
    Ok(blocks)
}

/// Parses every `@@ ... @@` hunk in a commit's `-L` diff block, reusing the
/// same hunk grammar [`parse_diff_hunk`] uses for an ordinary `git diff`
/// (the two are textually identical).
fn parse_line_history_hunks(lines: &[&str]) -> Result<Vec<DiffHunk>, GitSailError> {
    let mut hunks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("@@ ") {
            let (hunk, consumed) = parse_diff_hunk(lines, i)?;
            hunks.push(hunk);
            i += consumed;
        } else {
            i += 1;
        }
    }
    Ok(hunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

    /// EPIC-22/T-223/US-112 regression guard: this adapter must never set
    /// `GIT_TERMINAL_PROMPT=0` (or otherwise disable Git's own credential
    /// prompting) in a way that would hide an authentication failure behind
    /// a silent hang or an opaque error instead of Git's own diagnostic.
    /// `locale_env` is the *only* environment this adapter ever adds on top
    /// of the inherited parent environment (`ProcessRequest::env` besides it
    /// is always empty here), so asserting on it exhaustively is a genuine
    /// guarantee, not merely a spot check.
    #[test]
    fn adapter_never_overrides_git_terminal_prompt_handling() {
        let env = GitCliProvider::locale_env();
        assert!(
            env.iter().all(|(key, _)| key != "GIT_TERMINAL_PROMPT"),
            "GitCliProvider must leave Git's own terminal-prompt behavior untouched: {env:?}"
        );
        // Only locale pinning is added; nothing here ever provides
        // credentials or silences Git's own prompting.
        assert_eq!(
            env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            vec!["LC_ALL", "LANG"]
        );
    }

    /// A `StdError` double carrying pre-recorded `git` stderr, standing in
    /// for the `ProcessDiagnostic` a real `ProcessFailure` carries (that
    /// type is private to `runner`), so these tests exercise the real
    /// reclassification logic against real captured Git output without a
    /// process dependency.
    #[derive(Debug)]
    struct RawStderr(&'static str);

    impl fmt::Display for RawStderr {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "exit_code=Some(128) args=[] stderr={}", self.0)
        }
    }

    impl std::error::Error for RawStderr {}

    fn process_failure(stderr: &'static str) -> GitSailError {
        GitSailError::new(
            ErrorCode::ProcessFailure,
            "git process exited with a non-zero status",
        )
        .with_source(RawStderr(stderr))
    }

    #[test]
    fn classifies_missing_identity_as_invalid_repository_state() {
        // Captured verbatim from a real `git commit` with no configured
        // identity and auto-detection disabled.
        let err = process_failure(
            "Author identity unknown\n\n*** Please tell me who you are.\n\nfatal: no email was given and auto-detection is disabled\n",
        );

        let classified = classify_commit_failure(err);

        assert_eq!(classified.code(), ErrorCode::InvalidRepositoryState);
        assert!(classified.remediation().unwrap().contains("user.name"));
        assert!(
            classified.diagnostic().is_some(),
            "original diagnostic must be preserved"
        );
    }

    #[test]
    fn leaves_a_hook_failure_as_a_generic_process_failure_with_its_own_diagnostic() {
        let err = process_failure("blocked by hook\n");

        let classified = classify_commit_failure(err);

        // A hook can print anything; this adapter must not pretend to
        // understand it, only preserve it as a diagnostic (US-012 criterion
        // 2's "diagnóstico" requirement, without over-fitting to hook text).
        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
        assert!(classified
            .diagnostic()
            .unwrap()
            .to_string()
            .contains("blocked by hook"));
    }

    #[test]
    fn classifies_a_stale_hunk_as_operation_conflict() {
        let err =
            process_failure("error: patch failed: f.txt:1\nerror: f.txt: patch does not apply\n");

        let classified = classify_apply_failure(err);

        assert_eq!(classified.code(), ErrorCode::OperationConflict);
        assert!(classified.remediation().unwrap().contains("refresh"));
    }

    #[test]
    fn leaves_an_unrelated_apply_failure_unclassified() {
        let err = process_failure("fatal: unrecognized input\n");

        let classified = classify_apply_failure(err);

        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
    }

    fn modified_file_diff() -> FileDiff {
        FileDiff {
            path: PathBuf::from("a.txt"),
            previous_path: None,
            change_type: ChangeType::Modified,
            is_binary: false,
            truncated: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    DiffLine {
                        origin: DiffLineOrigin::Context,
                        content: "one".to_string(),
                        has_trailing_newline: true,
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Deletion,
                        content: "two".to_string(),
                        has_trailing_newline: true,
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Addition,
                        content: "TWO".to_string(),
                        has_trailing_newline: true,
                    },
                ],
            }],
        }
    }

    #[test]
    fn renders_a_modified_file_as_a_minimal_git_apply_patch() {
        let patch = render_hunk_patch(&[modified_file_diff()]);

        assert_eq!(
            patch,
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n"
        );
    }

    #[test]
    fn renders_an_added_file_against_dev_null() {
        let file = FileDiff {
            path: PathBuf::from("new.txt"),
            previous_path: None,
            change_type: ChangeType::Added,
            is_binary: false,
            truncated: false,
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![DiffLine {
                    origin: DiffLineOrigin::Addition,
                    content: "hello".to_string(),
                    has_trailing_newline: true,
                }],
            }],
        };

        let patch = render_hunk_patch(&[file]);

        assert!(patch.starts_with("--- /dev/null\n+++ b/new.txt\n"));
    }

    #[test]
    fn renders_a_deleted_file_against_dev_null() {
        let file = FileDiff {
            path: PathBuf::from("gone.txt"),
            previous_path: None,
            change_type: ChangeType::Deleted,
            is_binary: false,
            truncated: false,
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 0,
                new_lines: 0,
                lines: vec![DiffLine {
                    origin: DiffLineOrigin::Deletion,
                    content: "bye".to_string(),
                    has_trailing_newline: true,
                }],
            }],
        };

        let patch = render_hunk_patch(&[file]);

        assert!(patch.starts_with("--- a/gone.txt\n+++ /dev/null\n"));
    }

    #[test]
    fn skips_files_with_no_selected_hunks() {
        let mut file = modified_file_diff();
        file.hunks.clear();

        let patch = render_hunk_patch(&[file]);

        assert_eq!(patch, "");
    }

    #[test]
    fn renders_a_no_trailing_newline_marker_for_the_affected_line() {
        let mut file = modified_file_diff();
        file.hunks[0].lines.last_mut().unwrap().has_trailing_newline = false;

        let patch = render_hunk_patch(&[file]);

        assert_eq!(
            patch,
            "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n\\ No newline at end of file\n"
        );
    }

    /// T-227/US-116 criterion 1: a mutation losing the race against another
    /// Git process already holding `.git/index.lock` must surface a clear,
    /// classified error, never a generic process failure a caller cannot
    /// distinguish from any other unrelated `git` exit code.
    #[test]
    fn classifies_an_index_lock_conflict_as_repository_locked() {
        let err = process_failure(
            "fatal: Unable to create '/repo/.git/index.lock': File exists.\n\nAnother git process seems to be running in this repository, e.g.\nan editor opened by 'git commit'. Please make sure all processes\nare terminated then try again.\n",
        );

        let classified = classify_index_lock_conflict(err);

        assert_eq!(classified.code(), ErrorCode::RepositoryLocked);
        assert!(classified.remediation().unwrap().contains("retry"));
        assert!(
            classified.diagnostic().is_some(),
            "original diagnostic must be preserved"
        );
    }

    #[test]
    fn leaves_an_unrelated_process_failure_unclassified_by_the_lock_check() {
        let err = process_failure("fatal: not a git repository\n");

        let classified = classify_index_lock_conflict(err);

        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
    }

    /// T-226/US-115 criterion 2: cancellation during diff/blame parsing
    /// (i.e. after the Git subprocess has already exited) must be checked
    /// periodically, not only once, and must surface as a distinct `Err`
    /// rather than silently returning a truncated-but-`Ok` result
    /// (criterion 3).
    #[test]
    fn parse_diff_is_cancelled_responsively_across_many_files() {
        let mut raw = String::new();
        for i in 0..(CANCEL_CHECK_STRIDE * 3) {
            raw.push_str(&format!(
                "diff --git a/f{i}.txt b/f{i}.txt\nnew file mode 100644\n--- /dev/null\n+++ b/f{i}.txt\n@@ -0,0 +1,1 @@\n+line\n"
            ));
        }
        let cancel = CancellationToken::new();
        cancel.cancel();

        let err = parse_diff(&raw, &cancel).unwrap_err();

        assert_eq!(err.code(), ErrorCode::Cancelled);
    }

    #[test]
    fn parse_diff_succeeds_fully_when_never_cancelled() {
        let raw = "diff --git a/f.txt b/f.txt\nnew file mode 100644\n--- /dev/null\n+++ b/f.txt\n@@ -0,0 +1,1 @@\n+line\n";
        let cancel = CancellationToken::new();

        let diff = parse_diff(raw, &cancel).unwrap();

        assert_eq!(diff.files.len(), 1);
    }

    #[test]
    fn parse_blame_is_cancelled_responsively_across_many_lines() {
        let mut raw = String::new();
        for i in 0..(CANCEL_CHECK_STRIDE * 3) {
            raw.push_str(&format!(
                "deadbeefdeadbeefdeadbeefdeadbeefdeadbeef {n} {n} 1\nauthor Ada Lovelace\nauthor-mail <ada@example.com>\nauthor-time 0\nauthor-tz +0000\n\tline {n}\n",
                n = i + 1
            ));
        }
        let cancel = CancellationToken::new();
        cancel.cancel();

        let err = parse_blame(&raw, &cancel).unwrap_err();

        assert_eq!(err.code(), ErrorCode::Cancelled);
    }
}
