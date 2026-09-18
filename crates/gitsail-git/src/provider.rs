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

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use gitsail_application::{
    ApplyPatchResult, BlameRequest, CherryPickResult, CommitQuery, DiffRequest, LineHistoryRequest,
    MergeParentPolicy, MergeResult, Page, PatchPreview, Precondition, PullOutcome, RebaseAction,
    RebasePlan, RebasePlanEntry, RebaseResult, RepositoryReadPort, RepositoryWritePort, ResetMode,
    RevertResult, StashApplyOutcome, StashScope, TagAnnotation, WorktreeBranchSpec,
};
use gitsail_domain::{
    BisectOperation, Blame, BlameLine, BlameOrigin, Branch, BranchKind, BranchName, ChangeType,
    Commit, CommitHash, ConflictSide, ConflictSideContent, ConflictSides, ConflictStage,
    ConflictedFile, Decoration, Diff, DiffHunk, DiffLine, DiffLineOrigin, ErrorCode, FileChange,
    FileContentAtRevision, FileContentKind, FileDiff, FileStatusCode, GitSailError, GitTimestamp,
    HeadState, InProgressOperation, LineHistory, LineHistoryEntry, MergeOperation,
    OperationCapability, RebaseOperation, ReflogEntry, ReflogObjectState, Remote, RemoteUrl,
    Repository, RepositoryId, RepositoryStatus, SequencerOperation, ShortHash, Signature, Stash,
    Tag, TagKind, Worktree, WorktreeHead,
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

/// `git reflog show --format:` string, one `RECORD_SEP`-terminated record
/// per reflog entry (T-241/US-089), mirroring [`STASH_FORMAT`]'s own
/// convention exactly, including the same reason `%gd` is excluded: combined
/// with `--date=raw` (needed for the date field below), Git renders `%gd`'s
/// `N` as the *date* rather than the entry's position — verified empirically
/// against a real repository — so the index instead comes from this
/// record's position in `git reflog show`'s output, which is always
/// newest-first (`HEAD@{0}` first), exactly like `git stash list`'s own
/// output order.
const REFLOG_FORMAT: &str = "%H\u{1f}%gs\u{1f}%ad\u{1e}";

/// Number of `FIELD_SEP`-delimited fields in [`REFLOG_FORMAT`].
const REFLOG_FIELD_COUNT: usize = 3;

/// `git log --reverse --pretty=format:` string producing one
/// `RECORD_SEP`-terminated record per candidate commit for
/// [`RepositoryWritePort::plan_rebase`] (EPIC-17/T-236/US-084): full hash,
/// abbreviated hash, subject — everything [`RebasePlanEntry`] needs for
/// display, nothing more. Mirrors [`LOG_FORMAT`]/[`TAG_FORMAT`]'s own
/// `RECORD_SEP`-per-record convention (a subject can, in principle, contain
/// characters a plain one-line-per-record format would mishandle).
const REBASE_PLAN_FORMAT: &str = "%H\u{1f}%h\u{1f}%s\u{1e}";

/// Number of `FIELD_SEP`-delimited fields in [`REBASE_PLAN_FORMAT`].
const REBASE_PLAN_FIELD_COUNT: usize = 3;

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

    /// Resolves the real, shared `.git` directory via `git rev-parse
    /// --git-common-dir` (ADR-019; T-227/US-116 criterion 1), so two linked
    /// worktrees of the same repository — which report distinct
    /// [`Repository::root_path`]s but share one object database/refs/index
    /// lock namespace — resolve to the same directory instead of two
    /// independent ones. Falls back to `--absolute-git-dir` for older Git
    /// versions that lack `--git-common-dir` (added in Git 2.5), which is
    /// at least correct for a repository with no linked worktrees (the
    /// common case). Shared by [`RepositoryReadPort::lock_key`] (T-227/
    /// US-116) and [`RepositoryReadPort::detect_in_progress_operation`]
    /// (T-230/US-078), which both need the one real, shared Git directory
    /// rather than `repo.root_path`.
    fn git_common_dir(&self, repo: &Repository) -> Result<PathBuf, GitSailError> {
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

    /// The paths `git status` currently reports as unmerged (conflicted),
    /// with each one's [`ConflictStage`] derived from its index/worktree
    /// status pair (T-230/US-078 criterion 1). Reuses [`Self::status`]
    /// (`git status --porcelain=v2`) rather than a second, separate `git
    /// diff --name-only --diff-filter=U` call — both are equally valid per
    /// this story's acceptance criteria, and this avoids a redundant
    /// subprocess when a caller is about to fetch full status anyway.
    fn conflicted_files(&self, repo: &Repository) -> Result<Vec<ConflictedFile>, GitSailError> {
        let status = self.status(repo)?;
        status
            .files
            .into_iter()
            .filter(|file| file.change_type == ChangeType::Unmerged)
            .map(|file| {
                Ok(ConflictedFile {
                    stage: conflict_stage(file.index_status, file.worktree_status)?,
                    path: file.path,
                })
            })
            .collect()
    }

    /// Reads one index stage (1: base, 2: ours, 3: theirs) of `path` via
    /// `git show :<stage>:<path>` (T-232/US-080 criterion 2). See
    /// [`RepositoryReadPort::conflict_sides`] for the full contract.
    fn read_conflict_stage(
        &self,
        repo: &Repository,
        stage: u8,
        path: &Path,
    ) -> Result<ConflictSideContent, GitSailError> {
        let object = format!(":{stage}:{}", path.to_string_lossy());
        let args = vec!["show".to_string(), object];
        Ok(match self.try_run(args, &repo.root_path)? {
            None => ConflictSideContent::Absent,
            Some(output) => match classify_file_content(&output.stdout) {
                FileContentKind::Text(text) => ConflictSideContent::Text(text),
                FileContentKind::Binary => ConflictSideContent::Binary,
                FileContentKind::Missing => ConflictSideContent::Absent,
            },
        })
    }

    /// Like [`Self::run`], but merges `extra_env` on top of
    /// [`Self::locale_env`] instead of the locale pinning alone — used by
    /// [`Self::execute_rebase_plan`] to hand the controlled sequence-editor
    /// helper its `GIT_SEQUENCE_EDITOR`/`GITSAIL_REBASE_TODO_FILE`
    /// coordinates (T-236/US-084 criterion 3).
    fn run_with_env(
        &self,
        args: Vec<String>,
        cwd: &Path,
        extra_env: Vec<(String, String)>,
    ) -> Result<ProcessOutput, GitSailError> {
        let mut env = Self::locale_env();
        env.extend(extra_env);
        let request = ProcessRequest::new(args, cwd.to_path_buf()).with_env(env);
        self.runner
            .run(request, &CancellationToken::new())
            .map_err(classify_index_lock_conflict)
    }

    /// Refuses `action` up front, with an explicit, clear error, when the
    /// working tree has any uncommitted change (T-235/US-083 criterion 2):
    /// this is the "blocked, never silently stashed" half of that
    /// criterion — a caller that wants to proceed anyway is expected to
    /// commit, or create an *explicit*, visible stash first
    /// ([`RepositoryWritePort::create_stash`], EPIC-18), never something
    /// this port does on its own behind the scenes.
    fn require_clean_worktree(&self, repo: &Repository, action: &str) -> Result<(), GitSailError> {
        let status = RepositoryReadPort::status(self, repo)?;
        if !status.is_clean() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!("cannot {action}: the working tree has uncommitted changes"),
            )
            .with_remediation(
                "commit your changes, or create an explicit stash first (RepositoryWritePort::create_stash), then retry — this is never done automatically",
            ));
        }
        Ok(())
    }

    /// Determines which of `hashes` are still readable objects, via a single
    /// `git cat-file --batch-check` invocation fed every hash on stdin
    /// (T-241/US-089 criterion 3) — one process for the whole batch rather
    /// than one `git cat-file -e` per entry. Returns the subset that exists;
    /// [`Self::reflog`] treats anything not in the returned set as missing.
    /// An empty `hashes` never spawns a process at all (an empty reflog is a
    /// valid, ordinary state).
    fn object_existence(
        &self,
        cwd: &Path,
        hashes: &[CommitHash],
    ) -> Result<HashSet<String>, GitSailError> {
        if hashes.is_empty() {
            return Ok(HashSet::new());
        }
        let mut stdin = String::new();
        for hash in hashes {
            stdin.push_str(hash.as_str());
            stdin.push('\n');
        }
        let request = ProcessRequest::new(
            vec![
                "cat-file".to_string(),
                "--batch-check=%(objectname) %(objecttype)".to_string(),
            ],
            cwd.to_path_buf(),
        )
        .with_env(Self::locale_env())
        .with_stdin(stdin.into_bytes());
        let output = self
            .runner
            .run(request, &CancellationToken::new())
            .map_err(classify_index_lock_conflict)?;
        let stdout = Self::stdout_string(&output)?;
        Ok(parse_batch_check_existence(&stdout))
    }

    /// Refuses to start a new rebase-shaped mutation when another
    /// [`InProgressOperation`] is already pending, mirroring [`Self::merge`]'s
    /// own check exactly (T-230/US-078 criterion 3).
    fn require_no_pending_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        let existing = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
        if !existing.is_none() {
            return Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "a {} is already in progress",
                    existing.kind_label().unwrap_or("operation")
                ),
            )
            .with_remediation(
                "continue or abort the in-progress operation before starting a new rebase",
            ));
        }
        Ok(())
    }

    /// Locates the `gitsail-sequence-editor` helper binary (T-236/US-084
    /// criterion 3): the program `GIT_SEQUENCE_EDITOR` points at so
    /// [`Self::execute_rebase_plan`] never has to open a real interactive
    /// editor. Checked, in order:
    ///
    /// 1. `GITSAIL_SEQUENCE_EDITOR_BIN`, an explicit override for a packaged
    ///    deployment that ships this helper at a fixed, known location.
    /// 2. Next to the currently running executable
    ///    ([`std::env::current_exe`]) — where Cargo places a `[[bin]]`
    ///    target's own output, and thus where every consumer of this crate
    ///    (`gitsail-cli`, `gitsail-tui`, the Desktop sidecar) finds it
    ///    alongside itself in an ordinary `cargo build`.
    /// 3. One directory up from that — `cargo test` binaries live in
    ///    `target/<profile>/deps/`, one level *below* where `[[bin]]`
    ///    targets like this helper land (`target/<profile>/`).
    ///
    /// Fails clearly, rather than falling back to anything resembling a
    /// shell, when none of these resolve to a real file.
    fn sequence_editor_path() -> Result<PathBuf, GitSailError> {
        if let Some(path) = std::env::var_os("GITSAIL_SEQUENCE_EDITOR_BIN") {
            return Ok(PathBuf::from(path));
        }
        let exe_name = if cfg!(windows) {
            "gitsail-sequence-editor.exe"
        } else {
            "gitsail-sequence-editor"
        };
        let current = std::env::current_exe().map_err(|err| {
            GitSailError::new(
                ErrorCode::Internal,
                "could not determine the current executable's path",
            )
            .with_source(err)
        })?;
        let mut candidates = Vec::new();
        if let Some(dir) = current.parent() {
            candidates.push(dir.join(exe_name));
            if let Some(parent_dir) = dir.parent() {
                candidates.push(parent_dir.join(exe_name));
            }
        }
        candidates
            .into_iter()
            .find(|candidate| candidate.is_file())
            .ok_or_else(|| {
                GitSailError::new(
                    ErrorCode::Internal,
                    "could not locate the gitsail-sequence-editor helper binary",
                )
                .with_remediation(
                    "build the workspace so gitsail-sequence-editor compiles alongside this binary, or set GITSAIL_SEQUENCE_EDITOR_BIN to its path",
                )
            })
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

    /// Resolves the real, shared `.git` directory (ADR-019; T-227/US-116
    /// criterion 1; T-230/US-078: also the directory
    /// [`Self::detect_in_progress_operation`] reads `MERGE_HEAD`/
    /// `rebase-merge`/`CHERRY_PICK_HEAD`/etc. from), so two linked
    /// worktrees of the same repository — which report distinct
    /// [`Repository::root_path`]s but share one object database/refs/index
    /// lock namespace, and the same in-progress-operation state — resolve
    /// to the same directory instead of two independent ones. See
    /// [`Self::git_common_dir`] for the actual resolution.
    fn lock_key(&self, repo: &Repository) -> Result<PathBuf, GitSailError> {
        self.git_common_dir(repo)
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

    /// Lists `HEAD`'s reflog entries, newest (`HEAD@{0}`) first, via `git
    /// reflog show` (T-241/US-089 criterion 1). Read-only: this never runs
    /// `git reset`/`checkout`/anything else that would move `HEAD`, the
    /// index, or the working tree (History Editing Rules #10) — it only
    /// reads the reflog file Git itself already maintains.
    ///
    /// The reflog's own hash/message/date fields are read directly out of
    /// the reflog entry itself (never by re-deriving them from the commit
    /// object), so a listing never fails even for an entry whose object has
    /// since been pruned — Git's `reflog show`/`log -g` machinery reads the
    /// same way. Each entry's [`ReflogObjectState`] is then determined
    /// separately, in a single `git cat-file --batch-check` call over every
    /// listed hash at once (see [`Self::object_existence`]), rather than one
    /// process per entry — cheap even for a long reflog, and correct even
    /// though the ordinary case (entries still within Git's own reflog
    /// expiry window) never actually needs it: expiring a reflog entry
    /// normally removes the entry itself from `git reflog show`'s output
    /// (verified empirically — see this task's own session notes), so a
    /// listed-but-pruned entry is a rare, largely defensive case (e.g.
    /// external repository corruption) rather than the ordinary "old
    /// history" one — but this must still never crash or fail the whole
    /// query on it (US-089 criterion 3).
    ///
    /// An unborn `HEAD` (no commits yet) has no reflog at all — reported as
    /// an empty list, mirroring [`Self::commits`]'s own "unresolvable
    /// revision is a legitimate empty state" convention, never an error.
    fn reflog(&self, repo: &Repository) -> Result<Vec<ReflogEntry>, GitSailError> {
        let args = vec![
            "reflog".to_string(),
            "show".to_string(),
            "--date=raw".to_string(),
            format!("--format={REFLOG_FORMAT}"),
            "HEAD".to_string(),
        ];
        let Some(output) = self.try_run(args, &repo.root_path)? else {
            return Ok(Vec::new());
        };
        let stdout = Self::stdout_string(&output)?;
        let mut entries = parse_reflog_records(&stdout)?;

        let hashes: Vec<CommitHash> = entries.iter().map(|entry| entry.commit.clone()).collect();
        let existing = self.object_existence(&repo.root_path, &hashes)?;
        for entry in &mut entries {
            entry.object_state = if existing.contains(entry.commit.as_str()) {
                ReflogObjectState::Present
            } else {
                ReflogObjectState::Missing
            };
        }
        Ok(entries)
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

    /// Detects a merge, rebase, cherry-pick, revert, or bisect currently in
    /// progress (T-230/US-078) by re-reading the marker files/directories
    /// Git itself keeps under the real, shared `.git` directory (never any
    /// in-memory GitSail state — criterion 2), checked in the fixed
    /// precedence below. Git guarantees at most one of these is present at
    /// a time in ordinary use (e.g. a rebase's own conflict handling uses
    /// `CHERRY_PICK_HEAD`-like sequencer state under `rebase-merge`, never
    /// a top-level `MERGE_HEAD`), so this returns the first match rather
    /// than trying to detect an inconsistent combination.
    ///
    /// Known gap, documented rather than silently mis-detected: a plain
    /// `git am` (mailbox patch application, not a rebase) also creates
    /// `.git/rebase-apply`, distinguished from a rebase only by the
    /// presence of `rebase-apply/rebasing` (rebase) vs `rebase-apply/
    /// applying` (`am`). An `am` session in progress is therefore reported
    /// as [`InProgressOperation::None`] here — `git am` is not one of the
    /// operations this story's acceptance criteria list, and the type has
    /// no variant for it yet.
    fn detect_in_progress_operation(
        &self,
        repo: &Repository,
    ) -> Result<InProgressOperation, GitSailError> {
        require_worktree(repo, "detect in-progress operation")?;
        let git_dir = self.git_common_dir(repo)?;

        if git_dir.join("MERGE_HEAD").is_file() {
            let heads = read_commit_hashes(&git_dir.join("MERGE_HEAD"));
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::Merge(MergeOperation {
                heads,
                conflicted_files,
                capabilities: vec![OperationCapability::Continue, OperationCapability::Abort],
            }));
        }

        let rebase_merge = git_dir.join("rebase-merge");
        if rebase_merge.is_dir() {
            let interactive = rebase_merge.join("interactive").is_file();
            let onto = read_commit_hash(&rebase_merge.join("onto"));
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::Rebase(RebaseOperation {
                interactive,
                onto,
                conflicted_files,
                capabilities: vec![
                    OperationCapability::Continue,
                    OperationCapability::Skip,
                    OperationCapability::Abort,
                ],
            }));
        }

        let rebase_apply = git_dir.join("rebase-apply");
        if rebase_apply.is_dir() && rebase_apply.join("rebasing").is_file() {
            let onto = read_commit_hash(&rebase_apply.join("onto"));
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::Rebase(RebaseOperation {
                interactive: false,
                onto,
                conflicted_files,
                capabilities: vec![
                    OperationCapability::Continue,
                    OperationCapability::Skip,
                    OperationCapability::Abort,
                ],
            }));
        }

        if git_dir.join("CHERRY_PICK_HEAD").is_file() {
            let target = read_commit_hash(&git_dir.join("CHERRY_PICK_HEAD"));
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::CherryPick(SequencerOperation {
                target,
                conflicted_files,
                capabilities: vec![
                    OperationCapability::Continue,
                    OperationCapability::Skip,
                    OperationCapability::Abort,
                ],
            }));
        }

        if git_dir.join("REVERT_HEAD").is_file() {
            let target = read_commit_hash(&git_dir.join("REVERT_HEAD"));
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::Revert(SequencerOperation {
                target,
                conflicted_files,
                capabilities: vec![
                    OperationCapability::Continue,
                    OperationCapability::Skip,
                    OperationCapability::Abort,
                ],
            }));
        }

        if git_dir.join("BISECT_LOG").is_file() {
            let conflicted_files = self.conflicted_files(repo)?;
            return Ok(InProgressOperation::BisectRun(BisectOperation {
                conflicted_files,
                capabilities: vec![OperationCapability::Skip, OperationCapability::Abort],
            }));
        }

        Ok(InProgressOperation::None)
    }

    /// Reads `path`'s three conflict sides via `git show :1:<path>`/
    /// `:2:<path>`/`:3:<path>` (T-232/US-080 criterion 2) — Git's own index
    /// stage numbering for the common ancestor, "ours", and "theirs"
    /// respectively. A stage that does not exist for this file (e.g. no
    /// base for a file added independently on both sides) exits non-zero and
    /// is reported as [`ConflictSideContent::Absent`], mirroring
    /// [`Self::file_content`]'s "a non-zero exit only ever means the path is
    /// missing at that point" convention. Binary vs. text classification
    /// reuses [`classify_file_content`], the exact same heuristic
    /// `file_content` already applies.
    fn conflict_sides(
        &self,
        repo: &Repository,
        path: &Path,
    ) -> Result<ConflictSides, GitSailError> {
        require_worktree(repo, "read conflict sides")?;
        Ok(ConflictSides {
            path: path.to_path_buf(),
            base: self.read_conflict_stage(repo, 1, path)?,
            ours: self.read_conflict_stage(repo, 2, path)?,
            theirs: self.read_conflict_stage(repo, 3, path)?,
        })
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

    fn rename_branch(
        &self,
        repo: &Repository,
        old_name: &BranchName,
        new_name: &BranchName,
    ) -> Result<(), GitSailError> {
        // `--` ends option parsing before either positional (same rationale
        // as `switch_branch`/`create_branch`/`delete_branch` above): neither
        // a caller-supplied old nor new name is ever parsed as a `git
        // branch` option, even if it happens to look like one (US-024/
        // EPIC-22 US-110 convention). `-m`, never `-M`: a colliding
        // `new_name` is Git's own refusal, and this port never escalates
        // past it (US-024 criterion 2).
        let args = vec![
            "branch".to_string(),
            "-m".to_string(),
            "--".to_string(),
            old_name.as_str().to_string(),
            new_name.as_str().to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_rename_branch_failure)?;
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

    /// Fetches `remote` via `git fetch` (US-096). `--` ends option parsing
    /// before `remote` (same rationale as `create_branch`'s own `--`): a
    /// caller-supplied remote name that happens to look like a flag is
    /// rejected as an unresolvable remote rather than parsed as an option.
    /// Only `refs/remotes/<remote>/...` is ever touched — never the working
    /// tree or index (US-096 criterion 3), which this adapter cannot
    /// accidentally violate since `git fetch` itself has no working-tree
    /// effect. Network/authentication failures are reclassified by
    /// [`classify_remote_transport_failure`] into a clear, distinct
    /// [`ErrorCode`] (US-096 criterion 2); a timeout or cancellation from
    /// `cancel` passes through [`GitProcessRunner`](crate::runner::GitProcessRunner)
    /// unclassified, already distinct from either (T-215/US-100 criterion
    /// 1: never reclassified into a false reassurance either).
    fn fetch(
        &self,
        repo: &Repository,
        remote: &str,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let args = vec!["fetch".to_string(), "--".to_string(), remote.to_string()];
        self.run_cancellable(args, &repo.root_path, cancel)
            .map_err(classify_remote_transport_failure)?;
        Ok(())
    }

    /// Integrates `remote`'s tracked `branch` via `git fetch` + `git merge
    /// --ff-only` (US-097). Always fetches first (via [`Self::fetch`], so
    /// network/auth failures there are already classified identically),
    /// ensuring the remote-tracking ref this compares against reflects
    /// `remote`'s *current* state rather than whatever was last observed —
    /// this is what makes a retry after a failed/incomplete previous
    /// attempt reconsult reality instead of repeating a stale decision
    /// (T-215/US-100 criterion 2). `--end-of-options` (matching
    /// `resolve_revision`'s own use, not a bare `--`) ends option parsing
    /// before the remote-tracking ref name, which `git merge` treats as a
    /// revision, not a path.
    fn pull(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<PullOutcome, GitSailError> {
        require_worktree(repo, "pull")?;
        self.fetch(repo, remote, cancel)?;

        let remote_ref = format!("{remote}/{}", branch.as_str());
        let args = vec![
            "merge".to_string(),
            "--ff-only".to_string(),
            "--end-of-options".to_string(),
            remote_ref,
        ];
        let output = self
            .run_cancellable(args, &repo.root_path, cancel)
            .map_err(classify_pull_failure)?;
        let stdout = Self::stdout_string(&output)?;
        if stdout.contains("Already up to date") {
            Ok(PullOutcome::AlreadyUpToDate)
        } else {
            let new_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
            Ok(PullOutcome::FastForwarded { new_head })
        }
    }

    /// Publishes the local `branch` to `remote` via a plain `git push`
    /// (US-098), using an explicit `<branch>:<branch>` refspec so the
    /// target is never left to an ambiguous/implicit upstream (US-098
    /// criterion 1). Never passes `--force`: Git's own non-fast-forward
    /// refusal is reclassified by [`classify_push_failure`] into a clear
    /// [`ErrorCode::OperationConflict`] (US-098 criterion 2) rather than
    /// ever retried with force automatically. A network/authentication
    /// failure never reports a false success (US-098 criterion 3): this
    /// call either returns `Ok(())` because Git itself reported the push
    /// accepted, or an `Err` — there is no third, ambiguous outcome.
    fn push(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let refspec = format!("{0}:{0}", branch.as_str());
        let args = vec![
            "push".to_string(),
            "--".to_string(),
            remote.to_string(),
            refspec,
        ];
        self.run_cancellable(args, &repo.root_path, cancel)
            .map_err(classify_push_failure)?;
        Ok(())
    }

    /// Force-publishes rewritten history for `branch` to `remote` via `git
    /// push --force-with-lease=<branch>:<expected>` (US-099) — the explicit
    /// two-part lease form (not the bare `--force-with-lease`, which
    /// compares against whatever this repository's own remote-tracking ref
    /// last happened to record) so the compare-and-swap is against exactly
    /// the hash `expected_remote_head` captured (US-099 criterion 2), not a
    /// possibly-stale local cache of it. When the remote's real tip for
    /// `branch` no longer matches — another push landed there since —
    /// Git's own server-side refusal is reclassified by
    /// [`classify_force_push_failure`] into a clear
    /// [`ErrorCode::OperationConflict`]; this is never retried as an
    /// unconditional `--force` (US-099 criterion 3).
    fn force_push_with_lease(
        &self,
        repo: &Repository,
        remote: &str,
        branch: &BranchName,
        expected_remote_head: &Precondition<CommitHash>,
        cancel: &CancellationToken,
    ) -> Result<(), GitSailError> {
        let lease = format!(
            "{}:{}",
            branch.as_str(),
            expected_remote_head.expected().as_str()
        );
        let refspec = format!("{0}:{0}", branch.as_str());
        let args = vec![
            "push".to_string(),
            format!("--force-with-lease={lease}"),
            "--".to_string(),
            remote.to_string(),
            refspec,
        ];
        self.run_cancellable(args, &repo.root_path, cancel)
            .map_err(classify_force_push_failure)?;
        Ok(())
    }

    /// See [`RepositoryWritePort::preview_patch_application`]. Delegates to
    /// [`Self::check_patch_application`], discarding the `ErrorCode`
    /// [`Self::apply_patch`] additionally needs.
    fn preview_patch_application(
        &self,
        repo: &Repository,
        patch_text: &str,
    ) -> Result<PatchPreview, GitSailError> {
        self.check_patch_application(repo, patch_text)
            .map(|(preview, _rejection_code)| preview)
    }

    /// See [`RepositoryWritePort::apply_patch`]. Shares
    /// [`Self::check_patch_application`] with the preview, so the
    /// immediate re-check this performs right before writing anything can
    /// never classify a rejection differently than the preview a caller
    /// just showed a moment earlier.
    fn apply_patch(
        &self,
        repo: &Repository,
        patch_text: &str,
    ) -> Result<ApplyPatchResult, GitSailError> {
        let (preview, rejection_code) = self.check_patch_application(repo, patch_text)?;
        if let Some(code) = rejection_code {
            return Err(GitSailError::new(
                code,
                preview
                    .rejection_reason
                    .unwrap_or_else(|| "the patch cannot be applied".to_string()),
            )
            .with_remediation("refresh the patch and retry, or resolve the reported conflict"));
        }

        // A plain `git apply` — never `--cached`/`--index` (this writes the
        // working tree, distinct from `stage_hunks`'s index-only apply) and
        // never `--unsafe-paths` (US-030 DoD: a malicious patch must never
        // write outside the repository; verified empirically against a
        // real path-traversal, absolute-path, and symlink-escape patch —
        // see `gitsail-git`'s test suite). `--whitespace=nowarn` matches
        // `apply_hunk_selection`'s own choice: whitespace warnings are
        // noise here, never a reason to refuse an otherwise-valid patch.
        let args = vec![
            "apply".to_string(),
            "--whitespace=nowarn".to_string(),
            "-".to_string(),
        ];
        self.run_with_stdin(args, &repo.root_path, patch_text.as_bytes().to_vec())
            .map_err(|err| {
                let (code, reason) = classify_patch_check_failure(&err);
                GitSailError::new(code, reason)
                    .with_remediation("refresh the diff/patch and retry")
                    .with_source(err)
            })?;
        Ok(ApplyPatchResult {
            applied_files: preview.affected_files,
        })
    }

    /// See [`RepositoryWritePort::merge`]. Refuses up front when another
    /// [`InProgressOperation`] is already pending (T-230/US-078 criterion 3),
    /// re-reading real `.git/` state rather than trusting any cached flag —
    /// exactly like every other precondition check in this adapter
    /// ([`Self::amend_commit`]'s HEAD revalidation,
    /// [`Self::revalidate_stash_identity`]). `target_revision` is resolved to
    /// a concrete commit *before* merging so a bad target fails with the
    /// same clear [`ErrorCode::RepositoryNotFound`]
    /// [`RepositoryReadPort::resolve_revision`] already gives, rather than
    /// whatever opaque message `git merge` itself would produce for it.
    ///
    /// Fast-forward vs. merge-commit is distinguished structurally — by
    /// comparing the resulting `HEAD` against the target's commit resolved
    /// *before* merging — never by parsing `git merge`'s own (potentially
    /// locale-sensitive) "Fast-forward" text (US-079 criterion 2). A
    /// target that was already an ancestor of `HEAD` ("Already up to date")
    /// is reported as [`MergeResult::FastForwarded`] too, at the same
    /// (unchanged) `HEAD` — a degenerate but still accurate case: `HEAD` is,
    /// and remains, at the target.
    ///
    /// A conflict is never reported as a generic [`ErrorCode::ProcessFailure`]
    /// (US-079 criterion 2/3): when `git merge` exits non-zero, this
    /// re-inspects real `.git/` state via
    /// [`RepositoryReadPort::detect_in_progress_operation`] — mirroring this
    /// task's own "never presume, always re-check" discipline — and reports
    /// [`MergeResult::Conflict`] only when that confirms a pending merge with
    /// actual unmerged files; any other failure (e.g. local changes that
    /// would be overwritten) is classified by [`classify_merge_failure`]
    /// instead.
    fn merge(&self, repo: &Repository, target_revision: &str) -> Result<MergeResult, GitSailError> {
        require_worktree(repo, "merge")?;

        let existing = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
        if !existing.is_none() {
            return Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "a {} is already in progress",
                    existing.kind_label().unwrap_or("operation")
                ),
            )
            .with_remediation(
                "continue or abort the in-progress operation before starting a new merge",
            ));
        }

        let target_before = RepositoryReadPort::resolve_revision(self, repo, target_revision)?;

        // `-c core.editor=true` (a global option, must precede the
        // subcommand): a merge that needs a commit (a real merge commit, not
        // a fast-forward) would otherwise try to open an interactive editor
        // for the message, which a headless process has no terminal to
        // satisfy — mirrors `create_commit`/`amend_commit`'s own always-
        // explicit `-m`. `--no-edit` accepts Git's default merge message
        // outright (harmless, and a no-op, for a fast-forward). `--end-of-
        // options` (matching `pull`'s own `git merge --ff-only` call) ends
        // option parsing before `target_revision`, a caller-controlled
        // value that must never be parsed as a flag.
        let args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            "merge".to_string(),
            "--no-edit".to_string(),
            "--end-of-options".to_string(),
            target_revision.to_string(),
        ];

        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let head_after = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
                if head_after == target_before {
                    Ok(MergeResult::FastForwarded {
                        new_head: head_after,
                    })
                } else {
                    Ok(MergeResult::MergeCommitCreated { hash: head_after })
                }
            }
            Err(err) => {
                if err.code() == ErrorCode::ProcessFailure {
                    if let InProgressOperation::Merge(merge_op) =
                        RepositoryReadPort::detect_in_progress_operation(self, repo)?
                    {
                        if !merge_op.conflicted_files.is_empty() {
                            return Ok(MergeResult::Conflict {
                                files: merge_op.conflicted_files,
                            });
                        }
                    }
                }
                Err(classify_merge_failure(err))
            }
        }
    }

    /// See [`RepositoryWritePort::mark_conflict_resolved`]. A plain `git add
    /// -- <path>` (T-232/US-080 criterion 3): since Git 2.0, this also
    /// correctly stages a *deletion* for a previously-tracked path now
    /// missing from the working tree (the resolution a person chooses by
    /// deleting a conflicted file outright, e.g. for a delete/modify
    /// conflict), not only a content update — so this one call covers both
    /// "keep this content" and "keep it deleted" resolutions without
    /// needing to special-case `git rm`.
    fn mark_conflict_resolved(&self, repo: &Repository, path: &Path) -> Result<(), GitSailError> {
        require_worktree(repo, "mark conflict resolved")?;
        let args = vec![
            "add".to_string(),
            "--".to_string(),
            path.to_string_lossy().into_owned(),
        ];
        self.run(args, &repo.root_path)?;
        Ok(())
    }

    /// See [`RepositoryWritePort::take_conflict_side`]. `git checkout
    /// --ours`/`--theirs -- <path>` replaces the working-tree content with
    /// that side wholesale, then [`Self::mark_conflict_resolved`] stages it
    /// — the documented binary-conflict flow (T-232/US-080 criterion 3).
    fn take_conflict_side(
        &self,
        repo: &Repository,
        path: &Path,
        side: ConflictSide,
    ) -> Result<(), GitSailError> {
        require_worktree(repo, "take conflict side")?;
        let flag = match side {
            ConflictSide::Ours => "--ours",
            ConflictSide::Theirs => "--theirs",
        };
        let args = vec![
            "checkout".to_string(),
            flag.to_string(),
            "--".to_string(),
            path.to_string_lossy().into_owned(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_take_conflict_side_failure)?;
        RepositoryWritePort::mark_conflict_resolved(self, repo, path)
    }

    /// See [`RepositoryWritePort::continue_operation`]. Dispatches on
    /// whatever [`RepositoryReadPort::detect_in_progress_operation`]
    /// currently detects — generic across merge/rebase/cherry-pick/revert
    /// (T-233/US-081), refusing up front when nothing is pending, the
    /// detected operation's own [`OperationCapability`] set does not offer
    /// `Continue` (e.g. a bisect run), or conflicted files still remain
    /// (US-081 criterion 2). `-c core.editor=true` avoids ever opening an
    /// interactive editor for the resulting commit message, matching
    /// [`Self::merge`]'s own rationale.
    fn continue_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        require_worktree(repo, "continue operation")?;
        let current = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
        if current.is_none() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no merge, rebase, cherry-pick, or revert is currently in progress",
            )
            .with_remediation("there is nothing to continue"));
        }
        if !current.supports(OperationCapability::Continue) {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "a {} in progress does not support continue",
                    current.kind_label().unwrap_or("operation")
                ),
            ));
        }
        if current.has_conflicts() {
            return Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "{} conflicted file(s) still need to be resolved",
                    current.conflicted_files().len()
                ),
            )
            .with_remediation("mark every conflicted file resolved, then retry"));
        }

        let subcommand = match &current {
            InProgressOperation::Merge(_) => "merge",
            InProgressOperation::Rebase(_) => "rebase",
            InProgressOperation::CherryPick(_) => "cherry-pick",
            InProgressOperation::Revert(_) => "revert",
            InProgressOperation::BisectRun(_) | InProgressOperation::None => {
                unreachable!("already refused above: no Continue capability / nothing pending")
            }
        };
        let args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            subcommand.to_string(),
            "--continue".to_string(),
        ];
        self.run(args, &repo.root_path)
            .map_err(classify_continue_failure)?;
        Ok(())
    }

    /// See [`RepositoryWritePort::abort_operation`]. Dispatches on whatever
    /// is currently detected, matching [`Self::continue_operation`]'s own
    /// rationale; `git bisect reset` is the bisect-specific equivalent of
    /// `--abort` for every other operation kind.
    fn abort_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        require_worktree(repo, "abort operation")?;
        let current = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
        if current.is_none() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no merge, rebase, cherry-pick, or revert is currently in progress",
            )
            .with_remediation("there is nothing to abort"));
        }
        if !current.supports(OperationCapability::Abort) {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "a {} in progress does not support abort",
                    current.kind_label().unwrap_or("operation")
                ),
            ));
        }

        let args: Vec<String> = match &current {
            InProgressOperation::Merge(_) => vec!["merge".to_string(), "--abort".to_string()],
            InProgressOperation::Rebase(_) => vec!["rebase".to_string(), "--abort".to_string()],
            InProgressOperation::CherryPick(_) => {
                vec!["cherry-pick".to_string(), "--abort".to_string()]
            }
            InProgressOperation::Revert(_) => vec!["revert".to_string(), "--abort".to_string()],
            InProgressOperation::BisectRun(_) => vec!["bisect".to_string(), "reset".to_string()],
            InProgressOperation::None => unreachable!("already refused above"),
        };
        self.run(args, &repo.root_path)
            .map_err(classify_abort_failure)?;
        Ok(())
    }

    /// See [`RepositoryWritePort::rebase`]. A plain `git rebase <onto>`
    /// (T-235/US-083): refuses up front when another operation is already
    /// pending ([`Self::require_no_pending_operation`], mirroring
    /// [`Self::merge`]'s own check) or when the working tree is dirty
    /// ([`Self::require_clean_worktree`], US-083 criterion 2 — never an
    /// automatic, hidden `git stash`). A conflict is reported as
    /// [`RebaseResult::Conflict`], never a generic process failure, by
    /// re-inspecting real `.git/` state exactly like [`Self::merge`] already
    /// does for its own conflict case.
    fn rebase(&self, repo: &Repository, onto_revision: &str) -> Result<RebaseResult, GitSailError> {
        require_worktree(repo, "rebase")?;
        self.require_no_pending_operation(repo)?;
        self.require_clean_worktree(repo, "rebase")?;

        // `-c core.editor=true`: a plain rebase never needs a commit-message
        // editor for an ordinary pick (each replayed commit keeps its
        // original message), but this stays consistent with every other
        // multi-step mutation in this file that could, in principle, be
        // asked to combine commits (`--autosquash` is never passed here,
        // but nothing about this call assumes a caller cannot reconfigure
        // that) — matches [`Self::merge`]/[`Self::continue_operation`]'s own
        // rationale.
        let args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            "rebase".to_string(),
            "--end-of-options".to_string(),
            onto_revision.to_string(),
        ];

        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let new_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
                Ok(RebaseResult::Completed { new_head })
            }
            Err(err) => {
                if err.code() == ErrorCode::ProcessFailure {
                    if let InProgressOperation::Rebase(rebase_op) =
                        RepositoryReadPort::detect_in_progress_operation(self, repo)?
                    {
                        if !rebase_op.conflicted_files.is_empty() {
                            return Ok(RebaseResult::Conflict {
                                files: rebase_op.conflicted_files,
                            });
                        }
                    }
                }
                Err(classify_rebase_failure(err))
            }
        }
    }

    /// See [`RepositoryWritePort::skip_operation`]. Dispatches on whatever
    /// [`RepositoryReadPort::detect_in_progress_operation`] currently
    /// detects, mirroring [`Self::continue_operation`]/[`Self::abort_operation`]'s
    /// own rationale (T-235/US-083 criterion 3). A merge never offers `Skip`
    /// (T-230/US-078's own domain modeling — a merge has no further step to
    /// skip past), so this refuses it with a clear message containing
    /// "unsupported" rather than ever attempting `git merge --skip` (which
    /// does not exist). `git bisect skip` is a positional subcommand, not an
    /// `--skip` flag, hence its own arm below.
    fn skip_operation(&self, repo: &Repository) -> Result<(), GitSailError> {
        require_worktree(repo, "skip operation")?;
        let current = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
        if current.is_none() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                "no merge, rebase, cherry-pick, or revert is currently in progress",
            )
            .with_remediation("there is nothing to skip"));
        }
        if !current.supports(OperationCapability::Skip) {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "skip is unsupported for a {} in progress",
                    current.kind_label().unwrap_or("operation")
                ),
            ));
        }

        let args: Vec<String> = match &current {
            InProgressOperation::Rebase(_) => vec!["rebase".to_string(), "--skip".to_string()],
            InProgressOperation::CherryPick(_) => {
                vec!["cherry-pick".to_string(), "--skip".to_string()]
            }
            InProgressOperation::Revert(_) => vec!["revert".to_string(), "--skip".to_string()],
            InProgressOperation::BisectRun(_) => vec!["bisect".to_string(), "skip".to_string()],
            InProgressOperation::Merge(_) | InProgressOperation::None => {
                unreachable!("already refused above: no Skip capability / nothing pending")
            }
        };
        self.run(args, &repo.root_path)
            .map_err(classify_skip_failure)?;
        Ok(())
    }

    /// See [`RepositoryWritePort::plan_rebase`]. Reads the candidate commit
    /// range (`onto..HEAD`, oldest first) via a plain `git log --reverse`
    /// (T-236/US-084 criterion 1) — never mutates anything. `onto_revision`
    /// and `HEAD` are both resolved to concrete commit hashes up front and
    /// used exclusively as the range boundaries from then on, so the actual
    /// `git log` invocation never depends on a ref still resolving the same
    /// way (that is exactly what [`Self::execute_rebase_plan`] revalidates
    /// before ever applying the result).
    fn plan_rebase(
        &self,
        repo: &Repository,
        onto_revision: &str,
    ) -> Result<RebasePlan, GitSailError> {
        require_worktree(repo, "plan rebase")?;
        let onto = RepositoryReadPort::resolve_revision(self, repo, onto_revision)?;
        let branch_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;

        let args = vec![
            "log".to_string(),
            "--reverse".to_string(),
            format!("--pretty=format:{REBASE_PLAN_FORMAT}"),
            "--date=raw".to_string(),
            "--no-color".to_string(),
            format!("{}..{}", onto.as_str(), branch_head.as_str()),
        ];
        let output = self.run(args, &repo.root_path)?;
        let stdout = Self::stdout_string(&output)?;
        let entries = parse_rebase_plan_entries(&stdout)?;

        Ok(RebasePlan {
            onto_revision: onto_revision.to_string(),
            onto,
            branch_head,
            entries,
        })
    }

    /// See [`RepositoryWritePort::execute_rebase_plan`]. Full mechanism
    /// (T-236/US-084 criterion 3; T-237/US-085):
    ///
    /// 1. [`RebasePlan::validate`] — position/action invariants, pure, no
    ///    I/O.
    /// 2. Refuses when another operation is pending, or the working tree is
    ///    dirty (mirrors [`Self::rebase`]).
    /// 3. Revalidates `plan.onto`/`plan.branch_head` are still what
    ///    `plan.onto_revision`/`HEAD` resolve to right now (T-236/US-084
    ///    criterion 2) — refuses with
    ///    [`gitsail_domain::ErrorCode::OperationConflict`] otherwise, never
    ///    silently executing a plan built against an older state.
    /// 4. An empty plan (`onto` already contains every commit) is exactly
    ///    [`Self::rebase`]'s own degenerate no-op case — delegated to it
    ///    directly rather than duplicated.
    /// 5. Otherwise, renders the plan into Git's own todo-list syntax
    ///    ([`render_rebase_todo`]) into a fresh temporary file, points
    ///    `GIT_SEQUENCE_EDITOR` at the `gitsail-sequence-editor` helper
    ///    binary with `GITSAIL_REBASE_TODO_FILE` naming that file, and runs
    ///    `git rebase -i --onto <onto> <onto>`. Every [`RebaseAction::Reword`]
    ///    entry is translated to Git's own `edit` command rather than
    ///    `reword`: Git stops cleanly right after applying that commit,
    ///    without ever opening a message editor, and this method itself
    ///    then runs `git commit --amend -m <message>` (the message reaching
    ///    Git purely as a `-m` argv element — the same mechanism
    ///    [`Self::create_commit`]/[`Self::amend_commit`] already use, never
    ///    a shell) before resuming via [`Self::continue_operation`]. A loop
    ///    re-inspects real `.git/` state after every step (never presumes
    ///    success, matching [`Self::continue_operation`]'s own discipline)
    ///    to tell apart: the whole plan finished; a real conflict (returned
    ///    as [`RebaseResult::Conflict`], leaving `InProgressOperation::Rebase`
    ///    for continue/skip/abort); or another clean `edit` pause.
    ///
    /// Injection safety: nothing derived from repository content — a
    /// commit's subject, a `Reword` message, a branch name — is ever
    /// interpolated into a string a shell parses. The todo list's action
    /// keyword and commit hash (the only two tokens Git's own sequencer
    /// actually executes anything based on) come exclusively from this
    /// method's own [`RebasePlanEntry::action`]/`commit` fields, never from
    /// a subject or message; the subject is appended purely as a trailing,
    /// never-executed comment; and `gitsail-sequence-editor` itself performs
    /// nothing but a byte-for-byte file copy (see its own module doc). A
    /// dedicated test (`tests/t235_237_rebase.rs`) exercises this against a
    /// real commit whose subject is crafted to look like a shell injection
    /// attempt.
    fn execute_rebase_plan(
        &self,
        repo: &Repository,
        plan: &RebasePlan,
    ) -> Result<RebaseResult, GitSailError> {
        require_worktree(repo, "rebase")?;
        plan.validate()?;
        self.require_no_pending_operation(repo)?;

        let current_onto = RepositoryReadPort::resolve_revision(self, repo, &plan.onto_revision)?;
        if current_onto != plan.onto {
            return Err(GitSailError::new(
                ErrorCode::OperationConflict,
                format!(
                    "'{}' now resolves to a different commit than when this rebase plan was built",
                    plan.onto_revision
                ),
            )
            .with_remediation("rebuild the rebase plan against the current state, then retry"));
        }
        let current_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
        if current_head != plan.branch_head {
            return Err(GitSailError::new(
                ErrorCode::OperationConflict,
                "HEAD has moved since this rebase plan was built",
            )
            .with_remediation("rebuild the rebase plan against the current state, then retry"));
        }

        self.require_clean_worktree(repo, "rebase")?;

        if plan.entries.is_empty() {
            return RepositoryWritePort::rebase(self, repo, &plan.onto_revision);
        }

        let (todo_text, mut pending_rewords) = render_rebase_todo(&plan.entries);

        let temp_dir = RebaseTodoTempDir::new()?;
        std::fs::write(temp_dir.todo_path(), &todo_text).map_err(|err| {
            GitSailError::new(
                ErrorCode::Internal,
                "failed to write the rebase plan to a temporary file",
            )
            .with_source(err)
        })?;

        let sequence_editor = Self::sequence_editor_path()?;
        let extra_env = vec![
            (
                "GIT_SEQUENCE_EDITOR".to_string(),
                sequence_editor.to_string_lossy().into_owned(),
            ),
            (
                "GITSAIL_REBASE_TODO_FILE".to_string(),
                temp_dir.todo_path().to_string_lossy().into_owned(),
            ),
        ];
        let args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            "rebase".to_string(),
            "-i".to_string(),
            "--onto".to_string(),
            plan.onto.as_str().to_string(),
            "--end-of-options".to_string(),
            plan.onto.as_str().to_string(),
        ];

        let mut pending_err = self.run_with_env(args, &repo.root_path, extra_env).err();

        // Bounded defensively: at most one clean pause per entry in the
        // plan (every pause is one of this plan's own `Reword` entries),
        // plus one final iteration to observe completion — never an
        // unbounded loop even if repository state somehow never converges.
        let max_iterations = plan.entries.len() + 2;
        for _ in 0..max_iterations {
            if let Some(err) = pending_err.take() {
                if err.code() != ErrorCode::ProcessFailure {
                    return Err(classify_rebase_failure(err));
                }
                // Falls through: a `ProcessFailure` here never distinguishes
                // "real conflict" from "paused cleanly at an edit step" by
                // its exit code alone — real `.git/` state below does,
                // exactly like `Self::merge`/`Self::rebase` already decide
                // their own conflict case.
            }

            let current = RepositoryReadPort::detect_in_progress_operation(self, repo)?;
            match current {
                InProgressOperation::None => {
                    let new_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
                    return Ok(RebaseResult::Completed { new_head });
                }
                InProgressOperation::Rebase(rebase_op)
                    if !rebase_op.conflicted_files.is_empty() =>
                {
                    return Ok(RebaseResult::Conflict {
                        files: rebase_op.conflicted_files,
                    });
                }
                InProgressOperation::Rebase(_) => {
                    // Paused with nothing conflicted: this plan's own
                    // `Reword` -> `edit` translation is the only command
                    // here that ever produces a clean pause, so the next
                    // queued message is exactly the one this pause is for.
                    let Some(message) = pending_rewords.pop_front() else {
                        return Err(GitSailError::new(
                            ErrorCode::Internal,
                            "the rebase paused for an edit step this plan did not expect",
                        ));
                    };
                    self.run(
                        vec![
                            "commit".to_string(),
                            "--amend".to_string(),
                            "-m".to_string(),
                            message,
                        ],
                        &repo.root_path,
                    )
                    .map_err(classify_commit_failure)?;
                    pending_err = RepositoryWritePort::continue_operation(self, repo).err();
                }
                other => {
                    return Err(GitSailError::new(
                        ErrorCode::Internal,
                        format!(
                            "unexpected {} in progress while executing a rebase plan",
                            other.kind_label().unwrap_or("operation")
                        ),
                    ));
                }
            }
        }

        Err(GitSailError::new(
            ErrorCode::Internal,
            "rebase plan execution did not converge within the expected number of steps",
        ))
    }

    /// See [`RepositoryWritePort::cherry_pick`]. `git cherry-pick <commit>`
    /// (T-238/US-086): refuses up front when another operation is already
    /// pending ([`Self::require_no_pending_operation`], mirroring
    /// [`Self::merge`]/[`Self::rebase`]'s own check). When `commit` is a
    /// merge commit, `merge_parent` must be supplied explicitly (US-086
    /// criterion 2) — this port refuses *before ever invoking Git* rather
    /// than letting Git's own "-m option is required" refusal stand in for
    /// this port's own explicit, documented contract (see
    /// [`MergeParentPolicy`]'s doc); passing a policy against a non-merge
    /// commit is refused just as explicitly, since Git itself has no
    /// meaningful parent-2 to select there either. `-c core.editor=true`
    /// avoids ever opening an interactive editor for the resulting commit
    /// message, matching [`Self::merge`]'s own rationale. A conflict and an
    /// "already applied" (empty) result are both distinguished from an
    /// ordinary success by re-inspecting real `.git/` state afterward,
    /// exactly like [`Self::merge`]/[`Self::rebase`] already do for their
    /// own conflict case (US-086 criterion 3).
    fn cherry_pick(
        &self,
        repo: &Repository,
        commit: &CommitHash,
        merge_parent: Option<MergeParentPolicy>,
    ) -> Result<CherryPickResult, GitSailError> {
        require_worktree(repo, "cherry-pick")?;
        self.require_no_pending_operation(repo)?;

        let target = RepositoryReadPort::commit(self, repo, commit)?;
        let mut args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            "cherry-pick".to_string(),
        ];
        if target.is_merge() {
            let Some(policy) = merge_parent else {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    format!(
                        "{commit} is a merge commit: cherry-picking it requires an explicit merge parent policy"
                    ),
                )
                .with_remediation(
                    "pass MergeParentPolicy::FirstParent to cherry-pick this merge commit against its first parent, or choose a different, non-merge commit",
                ));
            };
            args.push("-m".to_string());
            args.push(policy.mainline_number().to_string());
        } else if merge_parent.is_some() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "{commit} is not a merge commit: a merge parent policy does not apply to it"
                ),
            ));
        }
        args.push("--end-of-options".to_string());
        args.push(commit.as_str().to_string());

        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let hash = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
                Ok(CherryPickResult::Applied { hash })
            }
            Err(err) => {
                if err.code() == ErrorCode::ProcessFailure {
                    let diagnostic_text =
                        err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
                    if diagnostic_text.contains("is now empty") {
                        return Ok(CherryPickResult::Empty);
                    }
                    if let InProgressOperation::CherryPick(op) =
                        RepositoryReadPort::detect_in_progress_operation(self, repo)?
                    {
                        if !op.conflicted_files.is_empty() {
                            return Ok(CherryPickResult::Conflict {
                                files: op.conflicted_files,
                            });
                        }
                    }
                }
                Err(classify_cherry_pick_failure(err))
            }
        }
    }

    /// See [`RepositoryWritePort::revert`]. `git revert <commit>` (T-239/
    /// US-087): a new commit undoing `commit`'s change, never a rewrite or
    /// move of any existing reference (History Editing Rules #8) — this is
    /// structural, not just a convention this port happens to follow: `git
    /// revert` only ever applies the inverse patch and creates a commit, the
    /// same code path `git cherry-pick` uses in the opposite direction, with
    /// no ref-moving/rewriting code path of its own. Otherwise mirrors
    /// [`Self::cherry_pick`] exactly: refuses up front when another
    /// operation is pending, requires an explicit `merge_parent` for a merge
    /// commit (US-087 criterion 3), and distinguishes a conflict from an
    /// ordinary success by re-inspecting real `.git/` state.
    fn revert(
        &self,
        repo: &Repository,
        commit: &CommitHash,
        merge_parent: Option<MergeParentPolicy>,
    ) -> Result<RevertResult, GitSailError> {
        require_worktree(repo, "revert")?;
        self.require_no_pending_operation(repo)?;

        let target = RepositoryReadPort::commit(self, repo, commit)?;
        let mut args = vec![
            "-c".to_string(),
            "core.editor=true".to_string(),
            "revert".to_string(),
        ];
        if target.is_merge() {
            let Some(policy) = merge_parent else {
                return Err(GitSailError::new(
                    ErrorCode::InvalidRepositoryState,
                    format!(
                        "{commit} is a merge commit: reverting it requires an explicit merge parent policy"
                    ),
                )
                .with_remediation(
                    "pass MergeParentPolicy::FirstParent to revert this merge commit against its first parent, or choose a different, non-merge commit",
                ));
            };
            args.push("-m".to_string());
            args.push(policy.mainline_number().to_string());
        } else if merge_parent.is_some() {
            return Err(GitSailError::new(
                ErrorCode::InvalidRepositoryState,
                format!(
                    "{commit} is not a merge commit: a merge parent policy does not apply to it"
                ),
            ));
        }
        args.push("--end-of-options".to_string());
        args.push(commit.as_str().to_string());

        match self.run(args, &repo.root_path) {
            Ok(_) => {
                let hash = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
                Ok(RevertResult::Applied { hash })
            }
            Err(err) => {
                if err.code() == ErrorCode::ProcessFailure {
                    if let InProgressOperation::Revert(op) =
                        RepositoryReadPort::detect_in_progress_operation(self, repo)?
                    {
                        if !op.conflicted_files.is_empty() {
                            return Ok(RevertResult::Conflict {
                                files: op.conflicted_files,
                            });
                        }
                    }
                }
                Err(classify_revert_failure(err))
            }
        }
    }

    /// See [`RepositoryWritePort::reset`]. `git reset --soft/--mixed/--hard
    /// <target>` (T-240/US-088). Refuses up front when another operation is
    /// already pending, mirroring [`Self::merge`]/[`Self::cherry_pick`]'s
    /// own check — resetting `HEAD`/the index mid-merge/mid-rebase would
    /// corrupt that operation's own state rather than cleanly abandon it (a
    /// caller that wants to abandon it uses
    /// [`Self::abort_operation`] instead). Revalidates `expected_head`
    /// against the current `HEAD` immediately before resetting (US-088
    /// criterion 3) via [`Precondition`], the same mechanism
    /// [`Self::amend_commit`] already uses — a `HEAD` that moved between
    /// preview and confirmation (e.g. a hard reset's reinforced
    /// confirmation, built against an older `HEAD`) is refused rather than
    /// executed against whatever `HEAD` happens to be now.
    ///
    /// `target_revision` is resolved to a concrete commit hash up front
    /// (rather than passed through as raw text with an `--end-of-options`
    /// guard, this adapter's usual injection-safety convention for a
    /// caller-controlled revision — see [`Self::merge`]/[`Self::rebase`]):
    /// verified empirically, `git reset` has no `--`/`--end-of-options`
    /// escape hatch compatible with `--soft`/`--mixed` at all (`--`
    /// switches `reset` into its own distinct "unstage these paths" mode,
    /// refused outright together with a mode flag: "Cannot do soft reset
    /// with paths"). Resolving first closes the same injection surface by
    /// construction instead: the argument `git reset` actually receives is
    /// always this adapter's own hex `CommitHash` text, never
    /// caller-controlled free text that could be parsed as a flag.
    fn reset(
        &self,
        repo: &Repository,
        target_revision: &str,
        mode: ResetMode,
        expected_head: &CommitHash,
    ) -> Result<(), GitSailError> {
        require_worktree(repo, "reset")?;
        self.require_no_pending_operation(repo)?;

        let current_head = RepositoryReadPort::resolve_revision(self, repo, "HEAD")?;
        Precondition::new(expected_head.clone())
            .revalidate(&current_head)
            .map_err(|_| {
                GitSailError::new(
                    ErrorCode::OperationConflict,
                    "HEAD changed since this reset was confirmed",
                )
                .with_remediation(
                    "review the new HEAD and this reset's predicted effect again before retrying",
                )
            })?;

        let target = RepositoryReadPort::resolve_revision(self, repo, target_revision)?;
        let args = vec![
            "reset".to_string(),
            mode.git_flag().to_string(),
            target.as_str().to_string(),
        ];
        self.run(args, &repo.root_path)?;
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

    /// Shared logic behind [`RepositoryWritePort::preview_patch_application`]
    /// and [`RepositoryWritePort::apply_patch`] (US-030). Both call this —
    /// the preview to report affected files/support without side effects,
    /// `apply_patch` to revalidate immediately before writing (Destructive
    /// Operations & Confirmation Guardrails rule 4) — so a rejection is
    /// always classified identically wherever it is checked.
    ///
    /// Three rejection categories are distinguished (US-030 criterion 2),
    /// in the order checked:
    /// 1. The patch has no recognizable file headers at all — `git apply`
    ///    itself is never even invoked for this case (DoD: a malformed or
    ///    malicious patch must never be executed as a command).
    /// 2. A declared path is unsafe — absolute, or containing a `..`
    ///    component — checked by this adapter itself, again *before*
    ///    `git apply` is invoked at all: this is on top of, never instead
    ///    of, Git's own refusal of the same thing ([`Self::apply_patch`]
    ///    never passes `--unsafe-paths`, so Git would refuse it too —
    ///    verified empirically, see this module's test suite — but
    ///    rejecting it here means a hostile patch is never handed to a
    ///    subprocess in the first place).
    /// 3. Anything `git apply --check` itself refuses: a malformed patch
    ///    body, an unsafe path this adapter's own check somehow missed, or
    ///    a hunk whose context no longer matches the current file content.
    ///
    /// Returns the built [`PatchPreview`] plus, when unsupported, the
    /// [`ErrorCode`] [`Self::apply_patch`] classifies the rejection as. The
    /// outer `Result` only ever fails for [`require_worktree`] (no working
    /// tree to apply into) — every patch-shaped rejection is a normal `Ok`
    /// with `supported: false`, never a [`GitSailError`] (mirrors
    /// [`StashApplyOutcome`]'s "a conflict is a legitimate, expected
    /// outcome" convention).
    fn check_patch_application(
        &self,
        repo: &Repository,
        patch_text: &str,
    ) -> Result<(PatchPreview, Option<ErrorCode>), GitSailError> {
        require_worktree(repo, "apply patch")?;
        let affected_files = declared_patch_paths(patch_text);

        if affected_files.is_empty() {
            return Ok((
                PatchPreview {
                    affected_files,
                    supported: false,
                    rejection_reason: Some(
                        "the patch text has no recognizable file headers".to_string(),
                    ),
                },
                Some(ErrorCode::ParseFailure),
            ));
        }

        if let Some(unsafe_path) = affected_files
            .iter()
            .find(|path| !is_safe_patch_path(path))
            .cloned()
        {
            return Ok((
                PatchPreview {
                    affected_files,
                    supported: false,
                    rejection_reason: Some(format!(
                        "the patch references a path outside the repository: {}",
                        unsafe_path.display()
                    )),
                },
                Some(ErrorCode::InvalidRepositoryState),
            ));
        }

        let args = vec![
            "apply".to_string(),
            "--check".to_string(),
            "--whitespace=nowarn".to_string(),
            "-".to_string(),
        ];
        match self.run_with_stdin(args, &repo.root_path, patch_text.as_bytes().to_vec()) {
            Ok(_) => Ok((
                PatchPreview {
                    affected_files,
                    supported: true,
                    rejection_reason: None,
                },
                None,
            )),
            Err(err) => {
                let (code, reason) = classify_patch_check_failure(&err);
                Ok((
                    PatchPreview {
                        affected_files,
                        supported: false,
                        rejection_reason: Some(reason),
                    },
                    Some(code),
                ))
            }
        }
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

/// Extracts the file paths a patch declares touching, straight from its
/// unified-diff headers (`--- a/<path>`/`+++ b/<path>`, tolerating a patch
/// with no `a/`/`b/` prefix, and a trailing tab-separated timestamp some
/// patch dialects append) — independent of whether `git apply` would
/// accept the patch at all, so a *rejected* patch still shows what it
/// claimed to touch (US-030 criterion 1: this is what makes a path-
/// traversal rejection's message name the exact offending path, rather
/// than a generic "invalid patch"). `/dev/null` — an added or deleted
/// file's other side — is never reported as an affected path. Order is
/// preserved and duplicates (a modified file has both a `---` and a `+++`
/// line naming the same path) are dropped.
fn declared_patch_paths(patch_text: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for line in patch_text.lines() {
        let Some(raw) = line
            .strip_prefix("+++ ")
            .or_else(|| line.strip_prefix("--- "))
        else {
            continue;
        };
        let raw = raw.split('\t').next().unwrap_or(raw).trim();
        if raw.is_empty() || raw == "/dev/null" {
            continue;
        }
        let stripped = raw
            .strip_prefix("a/")
            .or_else(|| raw.strip_prefix("b/"))
            .unwrap_or(raw);
        let candidate = PathBuf::from(stripped);
        if !paths.contains(&candidate) {
            paths.push(candidate);
        }
    }
    paths
}

/// Whether `path` is safe for `git apply` to touch inside a repository:
/// relative (never absolute) and free of any `..` component anywhere in
/// it. Checked by this adapter itself *before* `git apply`/`git apply
/// --check` is ever invoked for a patch declaring an unsafe path (US-030
/// DoD: a malicious patch must never even be executed as a command) — on
/// top of, never instead of, Git's own refusal of the same thing: this
/// adapter never passes `--unsafe-paths` to `git apply`, and Git itself
/// then independently refuses an absolute path, a `..`-containing path, or
/// a path reached through a symbolic link, all verified empirically (see
/// this module's test suite).
fn is_safe_patch_path(path: &Path) -> bool {
    path.is_relative()
        && !path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
}

/// Reclassifies a failed whole-patch `git apply --check`/`git apply`
/// invocation (distinct from [`classify_apply_failure`], which handles the
/// `--cached` hunk-level case) into one of the rejection categories US-030
/// criterion 2 names, with a message safe to show a user directly (the raw
/// Git diagnostic stays attached via the caller's `with_source`, never
/// folded into this message — SAD §19, §28). Any failure kind other than
/// `ProcessFailure` (e.g. `Timeout`, `Cancelled`) passes through unchanged.
fn classify_patch_check_failure(err: &GitSailError) -> (ErrorCode, String) {
    if err.code() != ErrorCode::ProcessFailure {
        return (err.code(), err.message().to_string());
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("does not apply") || diagnostic_text.contains("patch failed") {
        (
            ErrorCode::OperationConflict,
            "the patch no longer applies to the current file content".to_string(),
        )
    } else if diagnostic_text.contains("invalid path")
        || diagnostic_text.contains("beyond a symbolic link")
    {
        (
            ErrorCode::InvalidRepositoryState,
            "the patch references a path outside the repository".to_string(),
        )
    } else if diagnostic_text.contains("No valid patches in input")
        || diagnostic_text.contains("corrupt patch")
        || diagnostic_text.contains("patch fragment without header")
        || diagnostic_text.contains("unrecognized input")
    {
        (
            ErrorCode::ParseFailure,
            "the patch is malformed".to_string(),
        )
    } else {
        (
            ErrorCode::ProcessFailure,
            "git could not apply the patch".to_string(),
        )
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

/// Reclassifies a failed `git branch -m <old> <new>` when it failed because
/// `new` already names another branch (US-024 criterion 2: "colisão de
/// nome não sobrescreve a referência") or `old` does not name an existing
/// branch. Any other failure passes through unchanged.
fn classify_rename_branch_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("already exists") {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "a branch with that name already exists",
        )
        .with_remediation("choose a different new branch name")
        .with_source(err)
    } else if diagnostic_text.contains("no branch named") {
        GitSailError::new(ErrorCode::RepositoryNotFound, "no such branch")
            .with_remediation("verify the branch name")
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
// EPIC-19/T-211..T-215 (US-096..100): remote operation (fetch/pull/push/
// force-push-with-lease) failure reclassification.
// ---------------------------------------------------------------------

/// Reclassifies a failed remote-transport Git invocation (`fetch`, the
/// `fetch` half of `pull`, `push`, or `force_push_with_lease`) as
/// [`ErrorCode::AuthenticationRequired`] or [`ErrorCode::NetworkFailure`]
/// when Git's own diagnostic clearly indicates one of those two — US-096
/// criterion 2's "network/auth failure has a distinct, identifiable result,
/// not one generic failure for every case" applies identically to every
/// remote operation, so this is the one shared implementation each
/// operation-specific classifier below (`classify_pull_failure`,
/// `classify_push_failure`, `classify_force_push_failure`) delegates to
/// first, rather than repeating the same text matching four times
/// (T-215/US-100: consolidated once here rather than per-operation).
///
/// Deliberately conservative: only recognizes text patterns actually
/// produced by real Git invocations against a local `file://`/path remote
/// requiring credentials, an unresolvable host, or a nonexistent/
/// inaccessible repository (verified directly against real Git while this
/// was written) — patterns Git's own SSH/HTTPS transports and credential
/// prompting produce, not a guess. Any other failure — including a
/// legitimate non-fast-forward/lease rejection, which is not a transport
/// problem at all — passes through unclassified (`ErrorCode::ProcessFailure`)
/// for the caller's own, operation-specific reclassification. A
/// [`ErrorCode::Timeout`]/[`ErrorCode::Cancelled`] result is not a
/// `ProcessFailure` at all and is returned completely untouched here: this
/// adapter never claims a timed-out or cancelled remote operation left
/// "nothing changed" (T-215/US-100 criterion 1) — Git may have already
/// applied something server-side by the time the connection was cut, and
/// this function has no way to know either way.
fn classify_remote_transport_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();

    let looks_like_authentication_failure = diagnostic_text.contains("Authentication failed")
        || diagnostic_text.contains("could not read Username")
        || diagnostic_text.contains("could not read Password")
        || diagnostic_text.contains("Invalid username or")
        || diagnostic_text.contains("Permission denied (publickey)")
        || diagnostic_text.contains("Permission denied, please try again");
    if looks_like_authentication_failure {
        return GitSailError::new(
            ErrorCode::AuthenticationRequired,
            "the remote refused this operation for lack of valid credentials",
        )
        .with_remediation(
            "check the credentials, SSH key, or SSH agent configured for this remote, then retry",
        )
        .with_source(err);
    }

    let looks_unreachable = diagnostic_text.contains("Could not resolve host")
        || diagnostic_text.contains("Could not read from remote repository")
        || diagnostic_text.contains("does not appear to be a git repository")
        || diagnostic_text.contains("unable to access")
        || diagnostic_text.contains("Connection refused")
        || diagnostic_text.contains("Connection timed out")
        || diagnostic_text.contains("Network is unreachable")
        || diagnostic_text.contains("not found");
    if looks_unreachable {
        return GitSailError::new(
            ErrorCode::NetworkFailure,
            "the remote could not be reached, or does not exist",
        )
        .with_remediation("verify the remote's name/URL and your network connection, then retry")
        .with_source(err);
    }

    err
}

/// Reclassifies a failed `git merge --ff-only` (the second half of
/// [`GitCliProvider::pull`]) as [`ErrorCode::OperationConflict`] when it
/// failed because the local and remote branches have diverged — US-097
/// criterion 2's fixed fast-forward-only policy. Applies
/// [`classify_remote_transport_failure`] first, though in practice the
/// preceding `fetch` call already surfaces any transport failure before
/// this merge step ever runs; kept here too so this function alone is a
/// complete classifier for whatever `git merge --ff-only` itself can fail
/// with. Any other failure passes through unchanged.
fn classify_pull_failure(err: GitSailError) -> GitSailError {
    let err = classify_remote_transport_failure(err);
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("Not possible to fast-forward") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "local and remote branches have diverged; this version only supports fast-forward pulls",
        )
        .with_remediation(
            "merge or rebase manually to reconcile the diverged histories yourself, then retry — automatic merge/rebase recovery is out of scope for this version",
        )
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git push` as [`ErrorCode::OperationConflict`]
/// when the remote rejected it because it has commits this branch does not
/// (a non-fast-forward rejection) — US-098 criterion 2: this never falls
/// back to `--force` on its own. Any other failure passes through
/// [`classify_remote_transport_failure`] unchanged.
fn classify_push_failure(err: GitSailError) -> GitSailError {
    let err = classify_remote_transport_failure(err);
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("(fetch first)")
        || diagnostic_text.contains("Updates were rejected because the remote contains work")
        || diagnostic_text.contains("non-fast-forward")
    {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "push rejected: the remote has commits this branch does not have",
        )
        .with_remediation("pull to integrate the remote's changes, then retry the push")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git push --force-with-lease` as
/// [`ErrorCode::OperationConflict`] when the lease was refused — the
/// remote's real tip for this branch no longer matches the hash the caller
/// captured as `expected_remote_head`, i.e. another push landed there since
/// (US-099 criterion 2's compare-and-swap actually failing the compare).
/// Never reclassified into anything that would suggest retrying with an
/// unconditional `--force` (US-099 criterion 3: this port simply has no
/// such call to fall back to). Any other failure passes through
/// [`classify_remote_transport_failure`] unchanged.
fn classify_force_push_failure(err: GitSailError) -> GitSailError {
    let err = classify_remote_transport_failure(err);
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("(stale info)") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "force push refused: the remote branch has moved since this operation's expected state was captured",
        )
        .with_remediation(
            "fetch the remote's current state, review what changed, and retry only if you still intend to overwrite it",
        )
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git merge` (T-231/US-079) into a clear
/// [`ErrorCode::OperationConflict`] for the genuine (non-conflict) refusals
/// Git can give: local changes that would be overwritten, or unrelated
/// histories. A real conflict never reaches this function at all — the
/// caller ([`GitCliProvider::merge`]) intercepts that case first by
/// re-inspecting `.git/` state. Any other failure passes through unchanged.
fn classify_merge_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("Your local changes to the following files would be overwritten")
        || diagnostic_text.contains("Please commit your changes or stash them")
    {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "local changes would be overwritten by this merge",
        )
        .with_remediation("commit or stash your local changes first, then retry")
        .with_source(err)
    } else if diagnostic_text.contains("refusing to merge unrelated histories") {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "the target has no common history with the current branch",
        )
        .with_remediation("verify this is the reference you intend to merge")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git checkout --ours`/`--theirs -- <path>` (T-232/
/// US-080's binary-conflict flow) as [`ErrorCode::InvalidRepositoryState`]
/// when `path` is not actually conflicted (so has no such stage to take
/// from). Any other failure passes through unchanged.
fn classify_take_conflict_side_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("did not match any file")
        || diagnostic_text.contains("no such path in the working tree")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "the path is not currently conflicted",
        )
        .with_remediation("refresh the conflict list and retry against a currently conflicted path")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git <op> --continue` (T-233/US-081) as
/// [`ErrorCode::OperationConflict`] when Git itself still finds unresolved
/// conflicts (defense in depth: [`GitCliProvider::continue_operation`]
/// already checks this before ever invoking Git) or an empty resulting
/// commit. Any other failure passes through unchanged.
fn classify_continue_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("unmerged")
        || diagnostic_text.contains("You must edit all merge conflicts")
        || diagnostic_text.contains("fix conflicts")
    {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "unresolved conflicted files remain",
        )
        .with_remediation("mark every conflicted file resolved, then retry")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git <op> --abort`/`git bisect reset` (T-233/
/// US-081) as [`ErrorCode::InvalidRepositoryState`] when there is nothing to
/// abort. Any other failure passes through unchanged.
fn classify_abort_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("no operation in progress")
        || diagnostic_text.contains("There is no merge to abort")
        || diagnostic_text.contains("no rebase in progress")
        || diagnostic_text.contains("no cherry-pick in progress")
        || diagnostic_text.contains("no revert in progress")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "no operation is currently in progress",
        )
        .with_remediation("there is nothing to abort")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git rebase` (T-235/US-083) as
/// [`ErrorCode::InvalidRepositoryState`] when Git's own refusal is exactly
/// the dirty-working-tree case [`GitCliProvider::require_clean_worktree`]
/// already checks for up front (defense in depth: a race between that check
/// and this actual invocation, e.g. another process touching the working
/// tree in between). Any other failure passes through unchanged.
fn classify_rebase_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("cannot rebase: You have unstaged changes")
        || diagnostic_text.contains("cannot rebase: Your index contains uncommitted changes")
        || diagnostic_text.contains("Please commit or stash them")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "the working tree has uncommitted changes",
        )
        .with_remediation(
            "commit your changes, or create an explicit stash first, then retry — this is never done automatically",
        )
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git cherry-pick` (T-238/US-086) into a clear
/// [`ErrorCode::OperationConflict`] for the genuine (non-conflict, non-empty)
/// refusal Git can give: local changes that would be overwritten. A real
/// conflict or an empty result never reaches this function at all — the
/// caller ([`GitCliProvider::cherry_pick`]) intercepts both cases first.
/// Any other failure passes through unchanged.
fn classify_cherry_pick_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("Your local changes to the following files would be overwritten")
        || diagnostic_text.contains("Please commit your changes or stash them")
    {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "local changes would be overwritten by this cherry-pick",
        )
        .with_remediation("commit or stash your local changes first, then retry")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git revert` (T-239/US-087) into a clear,
/// classified error for the two genuine (non-conflict) refusals Git can
/// give: local changes that would be overwritten, and an empty result (the
/// commit's change is not present to undo). Verified empirically against
/// real Git 2.43: unlike [`GitCliProvider::cherry_pick`]'s own empty case
/// (which leaves a paused `CHERRY_PICK_HEAD` and prints "previous
/// cherry-pick is now empty"), a plain single `git revert`'s empty result
/// exits with Git's ordinary "nothing to commit, working tree clean" and
/// leaves **no** `REVERT_HEAD` behind at all — there is nothing left
/// pending to skip/abort in that case, so this is reported as a plain,
/// clearly classified error rather than implying a recoverable paused
/// operation exists. "is now empty" is still matched too, defensively, for
/// the (rarer) case for `git revert`'s own sequencer-pause path reports it
/// with that exact cherry-pick-shared wording instead. A real conflict
/// never reaches this function at all — the caller
/// ([`GitCliProvider::revert`]) intercepts that case first. Any other
/// failure passes through unchanged.
fn classify_revert_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("is now empty")
        || diagnostic_text.contains("nothing to commit, working tree clean")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "this revert would produce no changes: the commit's effect is not present on the current branch",
        )
        .with_remediation(
            "skip this revert (RepositoryWritePort::skip_operation) or abort it (RepositoryWritePort::abort_operation), if one is still pending, or simply choose a different commit",
        )
        .with_source(err)
    } else if diagnostic_text
        .contains("Your local changes to the following files would be overwritten")
        || diagnostic_text.contains("Please commit your changes or stash them")
    {
        GitSailError::new(
            ErrorCode::OperationConflict,
            "local changes would be overwritten by this revert",
        )
        .with_remediation("commit or stash your local changes first, then retry")
        .with_source(err)
    } else {
        err
    }
}

/// Reclassifies a failed `git <op> --skip`/`git bisect skip` (T-235/US-083)
/// as [`ErrorCode::InvalidRepositoryState`] when there is nothing to skip.
/// Any other failure passes through unchanged.
fn classify_skip_failure(err: GitSailError) -> GitSailError {
    if err.code() != ErrorCode::ProcessFailure {
        return err;
    }
    let diagnostic_text = err.diagnostic().map(|d| d.to_string()).unwrap_or_default();
    if diagnostic_text.contains("no rebase in progress")
        || diagnostic_text.contains("no cherry-pick in progress")
        || diagnostic_text.contains("no revert in progress")
        || diagnostic_text.contains("not currently")
    {
        GitSailError::new(
            ErrorCode::InvalidRepositoryState,
            "no operation is currently in progress",
        )
        .with_remediation("there is nothing to skip")
        .with_source(err)
    } else {
        err
    }
}

// ---------------------------------------------------------------------
// EPIC-17/T-236/US-084: interactive rebase plan execution mechanics —
// rendering the plan into a Git interactive-rebase todo list, and the
// temporary file the controlled `GIT_SEQUENCE_EDITOR` helper copies it from.
// See `gitsail_sequence_editor` (`src/bin/gitsail_sequence_editor.rs`) and
// `GitCliProvider::execute_rebase_plan`'s own doc for the full mechanism and
// why it is injection-safe.
// ---------------------------------------------------------------------

/// A freshly created, uniquely named temporary directory holding the
/// rendered rebase todo list, removed on drop regardless of how
/// [`GitCliProvider::execute_rebase_plan`] returns (success, conflict, or
/// error) — this is disposable coordination state, never anything Git
/// itself needs to keep.
struct RebaseTodoTempDir(PathBuf);

impl RebaseTodoTempDir {
    fn new() -> Result<Self, GitSailError> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "gitsail-rebase-plan-{}-{nanos}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).map_err(|err| {
            GitSailError::new(
                ErrorCode::Internal,
                "failed to create a temporary directory for the rebase plan",
            )
            .with_source(err)
        })?;
        Ok(Self(path))
    }

    fn todo_path(&self) -> PathBuf {
        self.0.join("todo")
    }
}

impl Drop for RebaseTodoTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Renders `entries` (in the exact order given — the plan's own order,
/// after any reordering a person has already applied) into Git's
/// interactive-rebase todo-list text, and separately collects every
/// [`RebaseAction::Reword`] entry's message, in the same top-to-bottom
/// order those entries appear — the order their `edit` stops actually occur
/// in once Git runs the plan (see [`GitCliProvider::execute_rebase_plan`]'s
/// doc for why `Reword` is translated to Git's own `edit` command rather
/// than `reword` here).
///
/// Every commit is addressed by its full hash, never a branch/tag name, so
/// nothing here depends on any ref still existing or meaning what it meant
/// when the plan was built. The subject appended after it is inert,
/// human-readable text as far as both this renderer and Git's own todo-list
/// parser are concerned (see [`GitCliProvider::execute_rebase_plan`]'s
/// module-level security doc) — still defensively flattened to a single
/// line here, since Git's own todo format is one command per line.
fn render_rebase_todo(entries: &[RebasePlanEntry]) -> (String, VecDeque<String>) {
    let mut todo = String::new();
    let mut reword_messages = VecDeque::new();
    for entry in entries {
        let command = match entry.action {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "edit",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        };
        if entry.action == RebaseAction::Reword {
            let message = entry.message_override.clone().expect(
                "RebasePlan::validate guarantees a Reword entry carries a message_override",
            );
            reword_messages.push_back(message);
        }
        let safe_subject = entry.subject.replace(['\n', '\r'], " ");
        todo.push_str(&format!(
            "{command} {} {safe_subject}\n",
            entry.commit.as_str()
        ));
    }
    (todo, reword_messages)
}

fn parse_rebase_plan_entries(raw: &str) -> Result<Vec<RebasePlanEntry>, GitSailError> {
    split_record_sep_blocks(raw)
        .map(parse_rebase_plan_entry)
        .collect()
}

fn parse_rebase_plan_entry(record: &str) -> Result<RebasePlanEntry, GitSailError> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != REBASE_PLAN_FIELD_COUNT {
        return Err(parse_err(
            "malformed git log record for a rebase plan: unexpected field count",
        ));
    }
    let commit = CommitHash::new(fields[0].to_string())?;
    let short_hash = ShortHash::new(fields[1].to_string())?;
    let subject = fields[2].to_string();
    Ok(RebasePlanEntry::pick(commit, short_hash, subject))
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
// T-230/US-078: in-progress-operation detection helpers.
// ---------------------------------------------------------------------

/// Reads a single commit hash from a `.git/` state file (e.g.
/// `CHERRY_PICK_HEAD`, `REVERT_HEAD`, `rebase-merge/onto`), taking only its
/// first line. Best-effort: a missing file, an I/O error, or content that
/// does not parse as a hex commit hash all report `None` rather than a
/// [`GitSailError`] — this is supplementary information, not something
/// that should ever block detecting *that* an operation is in progress.
fn read_commit_hash(path: &Path) -> Option<CommitHash> {
    let contents = std::fs::read_to_string(path).ok()?;
    let first_line = contents.lines().next()?.trim();
    CommitHash::new(first_line.to_string()).ok()
}

/// Reads zero or more commit hashes from a `.git/` state file, one per
/// line (`MERGE_HEAD` records more than one line for an octopus merge).
/// Best-effort per line, mirroring [`read_commit_hash`]: a missing file
/// reports an empty list, and a line that fails to parse as a commit hash
/// is skipped rather than failing the whole read.
fn read_commit_hashes(path: &Path) -> Vec<CommitHash> {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .filter_map(|line| CommitHash::new(line.trim().to_string()).ok())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Maps an unmerged path's `(index_status, worktree_status)` pair to its
/// [`ConflictStage`], mirroring Git's own seven unmerged `XY` status codes
/// (`git status --porcelain=v2`'s `u` line; see `git-status(1)`): `DD`
/// (both deleted), `AU` (added by us), `UD` (deleted by them), `UA` (added
/// by them), `DU` (deleted by us), `AA` (both added), `UU` (both
/// modified). Any other combination would mean Git itself emitted an
/// unmerged entry this adapter does not recognize — reported as
/// [`ErrorCode::ParseFailure`] rather than silently guessed.
fn conflict_stage(
    index: FileStatusCode,
    worktree: FileStatusCode,
) -> Result<ConflictStage, GitSailError> {
    use FileStatusCode::{Added, Deleted, UpdatedButUnmerged};
    match (index, worktree) {
        (Deleted, Deleted) => Ok(ConflictStage::BothDeleted),
        (Added, UpdatedButUnmerged) => Ok(ConflictStage::AddedByUs),
        (UpdatedButUnmerged, Deleted) => Ok(ConflictStage::DeletedByThem),
        (UpdatedButUnmerged, Added) => Ok(ConflictStage::AddedByThem),
        (Deleted, UpdatedButUnmerged) => Ok(ConflictStage::DeletedByUs),
        (Added, Added) => Ok(ConflictStage::BothAdded),
        (UpdatedButUnmerged, UpdatedButUnmerged) => Ok(ConflictStage::BothModified),
        (other_index, other_worktree) => Err(parse_err(format!(
            "unrecognized unmerged status combination ({other_index:?}, {other_worktree:?})"
        ))),
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
// `git reflog show` parsing (T-241/US-089).
// ---------------------------------------------------------------------

/// Parses `raw` `git reflog show --format=REFLOG_FORMAT` output into
/// entries, newest first, mirroring [`parse_stash_records`] exactly —
/// including leaving [`ReflogEntry::object_state`] at a placeholder value
/// ([`ReflogObjectState::Missing`]) here: [`GitCliProvider::reflog`] fills in
/// the real value afterward via a single batched existence check, rather
/// than this per-record parser doing it (which would mean one `git
/// cat-file` process per entry).
fn parse_reflog_records(raw: &str) -> Result<Vec<ReflogEntry>, GitSailError> {
    split_record_sep_blocks(raw)
        .enumerate()
        .map(|(index, record)| parse_reflog_record(index as u32, record))
        .collect()
}

fn parse_reflog_record(index: u32, record: &str) -> Result<ReflogEntry, GitSailError> {
    let fields: Vec<&str> = record.split(FIELD_SEP).collect();
    if fields.len() != REFLOG_FIELD_COUNT {
        return Err(parse_err(
            "malformed git reflog show record: unexpected field count",
        ));
    }
    let commit = CommitHash::new(fields[0].to_string())?;
    let message = fields[1].to_string();
    let date = parse_raw_date(fields[2])?;
    Ok(ReflogEntry {
        index,
        commit,
        message,
        date,
        // Overwritten by `GitCliProvider::reflog` right after this parses —
        // see this function's own doc.
        object_state: ReflogObjectState::Missing,
    })
}

/// Parses `git cat-file --batch-check='%(objectname) %(objecttype)'`
/// output (fed one hash per stdin line by [`GitCliProvider::object_existence`])
/// into the subset of hashes that still exist. A hash Git could not find
/// reports its type as the literal string `missing` (verified empirically
/// against a real `git cat-file --batch-check` run) rather than one of the
/// real object types (`commit`, `tag`, ...); anything else is treated as
/// existing regardless of its exact type, since [`GitCliProvider::reflog`]
/// only ever asks about commit hashes here. A line this adapter cannot even
/// parse into `<hash> <type>` is skipped rather than treated as either
/// outcome — defensive, since malformed output would indicate a Git version
/// mismatch this adapter does not otherwise support, not a real "missing"
/// or "present" answer.
fn parse_batch_check_existence(stdout: &str) -> HashSet<String> {
    stdout
        .lines()
        .filter_map(|line| {
            let (object, kind) = line.split_once(' ')?;
            (kind != "missing").then(|| object.to_string())
        })
        .collect()
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

    /// T-235/US-083 criterion 2 (defense in depth): a real `git rebase`
    /// refusal over a dirty working tree is reclassified clearly, distinct
    /// from a genuine conflict or an unrelated process failure.
    #[test]
    fn classifies_a_dirty_working_tree_rebase_refusal_as_invalid_repository_state() {
        let err = process_failure(
            "cannot rebase: You have unstaged changes.\nPlease commit or stash them.\n",
        );

        let classified = classify_rebase_failure(err);

        assert_eq!(classified.code(), ErrorCode::InvalidRepositoryState);
        assert!(classified.remediation().unwrap().contains("stash"));
    }

    #[test]
    fn leaves_an_unrelated_rebase_failure_unclassified() {
        let err = process_failure("fatal: unrecognized input\n");

        let classified = classify_rebase_failure(err);

        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
    }

    #[test]
    fn classifies_a_skip_with_nothing_pending_as_invalid_repository_state() {
        let err = process_failure("fatal: no rebase in progress?\n");

        let classified = classify_skip_failure(err);

        assert_eq!(classified.code(), ErrorCode::InvalidRepositoryState);
    }

    /// T-236/US-084 criterion 3: the todo list only ever carries the plan's
    /// own action keyword and commit hash — a maliciously crafted subject
    /// never becomes anything but a trailing, inert comment on its line.
    #[test]
    fn render_rebase_todo_treats_a_malicious_subject_as_inert_trailing_text() {
        let commit = CommitHash::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let short = ShortHash::new("aaaaaaa".to_string()).unwrap();
        let entry = RebasePlanEntry::pick(commit, short, "evil\"; rm -rf / #\ninjected\r\nmore");

        let (todo, rewords) = render_rebase_todo(&[entry]);

        assert!(rewords.is_empty());
        assert_eq!(
            todo,
            "pick aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa evil\"; rm -rf / # injected  more\n"
        );
        // Exactly one line: no embedded newline ever splits the todo list
        // into a second, unintended line/command.
        assert_eq!(todo.lines().count(), 1);
    }

    /// T-237/US-085 criterion 1: `Reword`'s message is queued in order and
    /// never leaks into the todo line itself (it is applied later via a
    /// dedicated `git commit --amend -m`, never through this file).
    #[test]
    fn render_rebase_todo_translates_reword_to_edit_and_queues_its_message() {
        let commit = CommitHash::new("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        let short = ShortHash::new("bbbbbbb".to_string()).unwrap();
        let mut entry = RebasePlanEntry::pick(commit, short, "original subject");
        entry.action = RebaseAction::Reword;
        entry.message_override = Some("a new message; rm -rf /".to_string());

        let (todo, mut rewords) = render_rebase_todo(&[entry]);

        assert!(todo.starts_with("edit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb "));
        assert!(!todo.contains("rm -rf"));
        assert_eq!(
            rewords.pop_front(),
            Some("a new message; rm -rf /".to_string())
        );
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

    // -----------------------------------------------------------------
    // EPIC-19/T-211..T-215 (US-096..100): remote operation reclassification.
    // -----------------------------------------------------------------

    /// Captured verbatim from a real `git fetch`/`push` against an HTTPS
    /// remote with `GIT_TERMINAL_PROMPT=0` and no credential helper
    /// configured.
    #[test]
    fn classifies_missing_credentials_as_authentication_required() {
        let err = process_failure(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n",
        );

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::AuthenticationRequired);
        assert!(classified.diagnostic().is_some());
    }

    /// Captured verbatim from a real rejected HTTPS push with a bad token.
    #[test]
    fn classifies_a_rejected_credential_as_authentication_required() {
        let err = process_failure(
            "remote: Invalid username or token. Password authentication is not supported for Git operations.\nfatal: Authentication failed for 'https://github.com/org/repo.git/'\n",
        );

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::AuthenticationRequired);
    }

    /// Captured verbatim from a real `git fetch` against an unresolvable
    /// hostname.
    #[test]
    fn classifies_an_unresolvable_host_as_network_failure() {
        let err = process_failure(
            "fatal: unable to access 'https://nonexistent.invalid.example/repo.git/': Could not resolve host: nonexistent.invalid.example\n",
        );

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::NetworkFailure);
    }

    /// Captured verbatim from a real `git fetch` against a nonexistent
    /// local path used as a remote (DoD: "fetch de remote inexistente ...
    /// comprova erro claro").
    #[test]
    fn classifies_a_nonexistent_local_remote_as_network_failure() {
        let err = process_failure(
            "fatal: '/no/such/path' does not appear to be a git repository\nfatal: Could not read from remote repository.\n",
        );

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::NetworkFailure);
    }

    #[test]
    fn leaves_an_unrelated_failure_unclassified_by_remote_transport_check() {
        let err = process_failure("fatal: some unrelated git failure\n");

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::ProcessFailure);
    }

    /// T-215/US-100 criterion 1: a timeout or cancellation must never be
    /// reclassified into anything else, and in particular must never gain
    /// wording implying a guaranteed rollback — it simply passes through.
    #[test]
    fn a_timeout_passes_through_remote_transport_classification_unchanged() {
        let err = GitSailError::new(ErrorCode::Timeout, "git process timed out after 30s");

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::Timeout);
        assert_eq!(classified.message(), "git process timed out after 30s");
    }

    #[test]
    fn a_cancellation_passes_through_remote_transport_classification_unchanged() {
        let err = GitSailError::new(ErrorCode::Cancelled, "git process was cancelled");

        let classified = classify_remote_transport_failure(err);

        assert_eq!(classified.code(), ErrorCode::Cancelled);
    }

    /// Captured verbatim from a real diverged `git merge --ff-only`.
    #[test]
    fn classifies_a_diverged_fast_forward_only_merge_as_operation_conflict() {
        let err = process_failure(
            "hint: Diverging branches can't be fast-forwarded, you need to either:\nfatal: Not possible to fast-forward, aborting.\n",
        );

        let classified = classify_pull_failure(err);

        assert_eq!(classified.code(), ErrorCode::OperationConflict);
        assert!(classified.remediation().unwrap().contains("rebase"));
    }

    #[test]
    fn pull_failure_classification_still_recognizes_transport_failures() {
        let err =
            process_failure("fatal: Authentication failed for 'https://example.com/repo.git/'\n");

        let classified = classify_pull_failure(err);

        assert_eq!(classified.code(), ErrorCode::AuthenticationRequired);
    }

    /// Captured verbatim from a real non-fast-forward `git push` rejection
    /// against a bare remote.
    #[test]
    fn classifies_a_non_fast_forward_push_rejection_as_operation_conflict() {
        let err = process_failure(
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs to '/tmp/remote'\nhint: Updates were rejected because the remote contains work that you do not\nhint: have locally.\n",
        );

        let classified = classify_push_failure(err);

        assert_eq!(classified.code(), ErrorCode::OperationConflict);
        assert!(classified.remediation().unwrap().contains("pull"));
    }

    #[test]
    fn push_failure_classification_still_recognizes_transport_failures() {
        let err = process_failure(
            "fatal: unable to access 'https://example.com/repo.git/': Could not resolve host: example.com\n",
        );

        let classified = classify_push_failure(err);

        assert_eq!(classified.code(), ErrorCode::NetworkFailure);
    }

    /// Captured verbatim from a real `--force-with-lease` rejection when the
    /// remote had already moved (DoD: "teste com um avanço concorrente do
    /// remote simulado").
    #[test]
    fn classifies_a_stale_lease_rejection_as_operation_conflict_never_suggesting_plain_force() {
        let err = process_failure(
            " ! [rejected]        main -> main (stale info)\nerror: failed to push some refs to '/tmp/remote'\n",
        );

        let classified = classify_force_push_failure(err);

        assert_eq!(classified.code(), ErrorCode::OperationConflict);
        let remediation = classified.remediation().unwrap().to_lowercase();
        assert!(
            !remediation.contains("--force ") && !remediation.contains("unconditional"),
            "a refused lease must never be advised to retry as an unconditional force push: {remediation:?}"
        );
    }

    #[test]
    fn force_push_failure_classification_still_recognizes_transport_failures() {
        let err = process_failure(
            "fatal: could not read Username for 'https://example.com': terminal prompts disabled\n",
        );

        let classified = classify_force_push_failure(err);

        assert_eq!(classified.code(), ErrorCode::AuthenticationRequired);
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

    // -------------------------------------------------------------------
    // T-241/US-089: `git reflog show`/`git cat-file --batch-check` parsing.
    // -------------------------------------------------------------------

    #[test]
    fn parse_reflog_records_reports_index_hash_message_and_date_in_output_order() {
        let raw = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{1f}commit: second\u{1f}200 +0000\u{1e}\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\u{1f}commit (initial): first\u{1f}100 +0000\u{1e}";

        let entries = parse_reflog_records(raw).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].index, 0);
        assert_eq!(
            entries[0].commit.as_str(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(entries[0].message, "commit: second");
        assert_eq!(entries[0].date.seconds_since_epoch, 200);
        assert_eq!(entries[1].index, 1);
        assert_eq!(entries[1].message, "commit (initial): first");
    }

    #[test]
    fn parse_reflog_records_rejects_a_malformed_record() {
        let raw = "onlyonefield\u{1e}";

        let err = parse_reflog_records(raw).unwrap_err();

        assert_eq!(err.code(), ErrorCode::ParseFailure);
    }

    /// T-241/US-089 criterion 3: an entry whose object no longer exists must
    /// be classified `Missing` rather than failing the whole batch, and this
    /// never depends on a real pruned repository to exercise (see
    /// `tests/t241_reflog.rs`'s own doc for why forcing that deterministically
    /// via real Git commands is not viable — `git reflog expire` removes the
    /// entry itself rather than leaving a dangling one).
    #[test]
    fn parse_batch_check_existence_classifies_missing_objects() {
        let stdout = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa commit\nbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb missing\n";

        let existing = parse_batch_check_existence(stdout);

        assert!(existing.contains("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(!existing.contains("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[test]
    fn parse_batch_check_existence_on_empty_output_reports_nothing_existing() {
        assert!(parse_batch_check_existence("").is_empty());
    }
}
