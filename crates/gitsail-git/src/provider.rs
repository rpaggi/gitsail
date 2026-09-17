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

use gitsail_application::{CommitQuery, DiffRequest, Page, RepositoryReadPort, RepositoryWritePort};
use gitsail_domain::{
    Blame, BlameLine, Branch, BranchKind, BranchName, ChangeType, Commit, CommitHash, Decoration,
    Diff, DiffHunk, DiffLine, DiffLineOrigin, ErrorCode, FileChange, FileDiff, FileStatusCode,
    GitSailError, GitTimestamp, HeadState, Repository, RepositoryId, RepositoryStatus, ShortHash,
    Signature,
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

    /// Runs `args` in `cwd`, always pinning the `C` locale. The trait this
    /// adapter implements does not (yet) expose cancellation, so a fresh,
    /// never-cancelled token is used for every call (SAD §39 notes this as
    /// a future evolution point).
    fn run(&self, args: Vec<String>, cwd: &Path) -> Result<ProcessOutput, GitSailError> {
        let request = ProcessRequest::new(args, cwd.to_path_buf()).with_env(Self::locale_env());
        self.runner.run(request, &CancellationToken::new())
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
            _ => return Err(parse_err("could not parse git rev-parse --is-bare-repository output")),
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

        let worktree_path = if is_bare { None } else { Some(root_path.clone()) };
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
        let revision = query
            .branch
            .as_ref()
            .map(BranchName::as_str)
            .or(query.revision_range.as_deref())
            .unwrap_or("HEAD");
        args.push(revision.to_string());
        if let Some(path) = &query.path_filter {
            // `--follow` only makes sense (and is only accepted by Git)
            // together with a single pathspec, which `path_filter` already
            // guarantees.
            if query.follow_renames {
                args.push("--follow".to_string());
            }
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

    fn diff(&self, repo: &Repository, request: &DiffRequest) -> Result<Diff, GitSailError> {
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

        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        parse_diff(&stdout)
    }

    fn blame(
        &self,
        repo: &Repository,
        file: &Path,
        revision: Option<&CommitHash>,
    ) -> Result<Blame, GitSailError> {
        let mut args = vec!["blame".to_string(), "--porcelain".to_string()];
        if let Some(rev) = revision {
            args.push(rev.as_str().to_string());
        }
        args.push("--".to_string());
        args.push(file.to_string_lossy().into_owned());

        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        parse_blame(&stdout)
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
        if matches!(self.determine_head_state(&repo.root_path)?, HeadState::Unborn) {
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
                let hash_output =
                    self.run(vec!["rev-parse".to_string(), "HEAD".to_string()], &repo.root_path)?;
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
        let args = vec!["switch".to_string(), target.as_str().to_string()];
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
        let mut args = vec!["branch".to_string(), name.as_str().to_string()];
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
        let args = vec![
            "branch".to_string(),
            flag.to_string(),
            name.as_str().to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_delete_branch_failure)?;
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
        let request = ProcessRequest::new(args, cwd.to_path_buf())
            .with_env(Self::locale_env())
            .with_stdin(stdin);
        self.runner.run(request, &CancellationToken::new())
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
        let mut args = vec!["apply".to_string(), "--cached".to_string(), "--whitespace=nowarn".to_string()];
        if direction == ApplyDirection::Reverse {
            args.push("--reverse".to_string());
        }
        args.push("-".to_string());

        self.run_with_stdin(args, &repo.root_path, patch.into_bytes())
            .map_err(classify_apply_failure)?;
        Ok(())
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
        .with_remediation("switch to a different branch, or a different worktree, before deleting it")
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

// ---------------------------------------------------------------------
// Unified diff patch rendering for hunk-level stage/unstage (US-013).
// ---------------------------------------------------------------------

/// Renders `selection` into `git apply`-compatible unified diff text: the
/// minimal `---`/`+++`/`@@` framing `git apply` accepts, without a
/// `diff --git`/`index` header (none of that is needed to apply content
/// hunks, and this adapter never needs to *parse* what it renders here).
///
/// Known limitation, inherited from how `diff()` parses hunks in the first
/// place: a line without a trailing newline at end-of-file is rendered with
/// one anyway, since [`DiffLine`] has no field to record its absence
/// (US-014 is where line-accurate newline handling is tracked).
fn render_hunk_patch(selection: &[FileDiff]) -> String {
    let mut out = String::new();
    for file in selection {
        if file.hunks.is_empty() {
            continue;
        }
        let (old_path, new_path) = patch_paths(file);
        out.push_str(&format!("--- {old_path}\n"));
        out.push_str(&format!("+++ {new_path}\n"));
        for hunk in &file.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                let sigil = match line.origin {
                    DiffLineOrigin::Context => ' ',
                    DiffLineOrigin::Addition => '+',
                    DiffLineOrigin::Deletion => '-',
                };
                out.push(sigil);
                out.push_str(&line.content);
                out.push('\n');
            }
        }
    }
    out
}

fn patch_paths(file: &FileDiff) -> (String, String) {
    match file.change_type {
        ChangeType::Added => (
            "/dev/null".to_string(),
            format!("b/{}", file.path.to_string_lossy()),
        ),
        ChangeType::Deleted => (
            format!("a/{}", file.path.to_string_lossy()),
            "/dev/null".to_string(),
        ),
        _ => (
            format!(
                "a/{}",
                file.previous_path.as_ref().unwrap_or(&file.path).to_string_lossy()
            ),
            format!("b/{}", file.path.to_string_lossy()),
        ),
    }
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
        Some(b'?') => parse_marker_status_entry(
            segment,
            FileStatusCode::Untracked,
            ChangeType::Untracked,
        )
        .map(Some),
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
        return Err(parse_err("unrecognized ref namespace in for-each-ref output"));
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
// `git diff` unified patch parsing.
// ---------------------------------------------------------------------

fn parse_diff(raw: &str) -> Result<Diff, GitSailError> {
    let files = split_diff_blocks(raw)
        .into_iter()
        .map(parse_diff_block)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Diff { files })
}

/// Splits a unified patch into per-file blocks, each starting with its
/// `diff --git a/... b/...` header line. Any bytes before the first such
/// header (not expected in practice) are discarded rather than misread.
fn split_diff_blocks(raw: &str) -> Vec<Vec<&str>> {
    let mut blocks: Vec<Vec<&str>> = Vec::new();
    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            blocks.push(vec![line]);
        } else if let Some(block) = blocks.last_mut() {
            block.push(line);
        }
    }
    blocks
}

fn parse_diff_block(lines: Vec<&str>) -> Result<FileDiff, GitSailError> {
    let header = *lines
        .first()
        .ok_or_else(|| parse_err("empty diff block"))?;

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

    let mut i = 1;
    while i < lines.len() {
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
            hunks.push(hunk);
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

    let mut content_lines = Vec::new();
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
            // "\ No newline at end of file" and any other stray marker
            // line: not a content line, skip without ending the hunk.
            _ => {
                i += 1;
                continue;
            }
        };
        content_lines.push(DiffLine {
            origin,
            content: line[1..].to_string(),
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

fn parse_blame(raw: &str) -> Result<Blame, GitSailError> {
    let mut metadata: HashMap<String, (Signature, GitTimestamp)> = HashMap::new();
    let mut lines_out = Vec::new();

    let mut current_hash: Option<String> = None;
    let mut current_final_line = 0u32;
    let mut current_original_line = 0u32;

    let mut pending_author_name: Option<String> = None;
    let mut pending_author_email: Option<String> = None;
    let mut pending_author_time: Option<i64> = None;
    let mut pending_author_tz: Option<i32> = None;

    for line in raw.split('\n') {
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
            lines_out.push(BlameLine {
                final_line: current_final_line,
                original_line: current_original_line,
                commit: CommitHash::new(hash)?,
                author,
                timestamp,
                content: content.to_string(),
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

    Ok(Blame { lines: lines_out })
}

fn strip_angle_brackets(raw: &str) -> String {
    raw.trim_start_matches('<').trim_end_matches('>').to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt;

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
        GitSailError::new(ErrorCode::ProcessFailure, "git process exited with a non-zero status")
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
        assert!(classified.diagnostic().is_some(), "original diagnostic must be preserved");
    }

    #[test]
    fn leaves_a_hook_failure_as_a_generic_process_failure_with_its_own_diagnostic() {
        let err = process_failure("blocked by hook\n");

        let classified = classify_commit_failure(err);

        // A hook can print anything; this adapter must not pretend to
        // understand it, only preserve it as a diagnostic (US-012 criterion
        // 2's "diagnóstico" requirement, without over-fitting to hook text).
        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
        assert!(classified.diagnostic().unwrap().to_string().contains("blocked by hook"));
    }

    #[test]
    fn classifies_a_stale_hunk_as_operation_conflict() {
        let err = process_failure("error: patch failed: f.txt:1\nerror: f.txt: patch does not apply\n");

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
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 2,
                lines: vec![
                    DiffLine {
                        origin: DiffLineOrigin::Context,
                        content: "one".to_string(),
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Deletion,
                        content: "two".to_string(),
                    },
                    DiffLine {
                        origin: DiffLineOrigin::Addition,
                        content: "TWO".to_string(),
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
            hunks: vec![DiffHunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 1,
                lines: vec![DiffLine {
                    origin: DiffLineOrigin::Addition,
                    content: "hello".to_string(),
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
            hunks: vec![DiffHunk {
                old_start: 1,
                old_lines: 1,
                new_start: 0,
                new_lines: 0,
                lines: vec![DiffLine {
                    origin: DiffLineOrigin::Deletion,
                    content: "bye".to_string(),
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
}
