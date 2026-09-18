//! Controlled `GIT_SEQUENCE_EDITOR` helper (T-236/US-084 criterion 3).
//!
//! `GitCliProvider::execute_rebase_plan` (`../provider.rs`) points Git's
//! `GIT_SEQUENCE_EDITOR` at this binary's own path so it never has to open a
//! real interactive editor to drive an interactive rebase — Git invokes it
//! with the path to its freshly generated `git-rebase-todo` file as the sole
//! argument, expecting the invoked program to leave the file it wants
//! applied at that same path when it exits.
//!
//! This program is deliberately as small and inert as possible, because it
//! is the one piece of this feature that sits between "a commit message or
//! branch name someone controls" and a process Git will actually execute
//! the result of:
//!
//! - It never spawns a shell, and never spawns anything at all.
//! - It takes exactly one meaningful input from its environment
//!   (`GITSAIL_REBASE_TODO_FILE`, a path `GitCliProvider` itself created in a
//!   fresh temporary directory just before invoking `git rebase -i` — never
//!   anything derived from repository content) and one from its argument
//!   list (the path Git wants written — Git's own temp file, also never
//!   repository content).
//! - Its only action is a byte-for-byte file copy from the former to the
//!   latter. It never parses, interprets, or executes either file's
//!   contents in any way — the *contents* of the todo list (including every
//!   commit subject `GitCliProvider` embedded in it as a comment) are only
//!   ever data as far as this program is concerned, and Git's own
//!   interactive-rebase parser in turn only ever reads the leading
//!   `<action> <sha>` token off each line, treating the rest as a
//!   human-readable label it never executes either.
//!
//! This is why a commit message or branch name crafted to look like a shell
//! command (e.g. containing `; rm -rf /`) can never become a command here:
//! there is no shell, and no code path in this program ever treats any byte
//! of that text as anything other than a byte to copy.
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os();
    let _argv0 = args.next();
    let Some(destination) = args.next() else {
        eprintln!(
            "gitsail-sequence-editor: expected exactly one argument (the file Git wants written), got none"
        );
        return ExitCode::FAILURE;
    };

    let Some(source) = std::env::var_os("GITSAIL_REBASE_TODO_FILE") else {
        eprintln!("gitsail-sequence-editor: GITSAIL_REBASE_TODO_FILE is not set");
        return ExitCode::FAILURE;
    };

    match std::fs::copy(&source, &destination) {
        Ok(_) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!(
                "gitsail-sequence-editor: failed to copy {source:?} -> {destination:?}: {err}"
            );
            ExitCode::FAILURE
        }
    }
}
