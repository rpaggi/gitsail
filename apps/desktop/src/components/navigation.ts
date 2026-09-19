// The sidebar's view registry — the single source of truth for what the
// nav offers, in what order, and which of those views mean anything
// without a repository open.
//
// Pure data + one pure resolver, so "clicking Branches shows the branches
// view" and "Settings stays usable with no repository open" are unit
// testable without mounting the shell. `AppShell.vue` renders one panel
// per id and nothing else decides navigation.

export type ViewId =
  | "overview"
  | "commits"
  | "branches"
  | "stashes"
  | "pull-requests"
  | "issues"
  | "files"
  | "diff"
  | "blame"
  | "tags"
  | "remotes"
  | "settings";

export interface NavItem {
  id: ViewId;
  label: string;
  /** `GsIcon.vue` glyph name. */
  icon: string;
  /** The `<main>` heading and subtitle this view renders. */
  title: string;
  subtitle: string;
  /**
   * Whether the view needs an open repository to show anything real.
   *
   * `false` for Settings (theme, shortcuts and updates are app-global
   * preferences, and the repository picker itself lives there — locking it
   * behind an open repository would make a failed open unrecoverable) and
   * for Issues (which has nothing to show either way; see `available`).
   */
  requiresRepository: boolean;
  /**
   * `false` for a nav entry with no implementation behind it. Only Issues
   * is unavailable today: GitSail has no issues store or service, and the
   * honest thing is to say so in the view rather than drop the entry and
   * pretend the product never intended it, or fill it with sample data.
   */
  available: boolean;
}

export const NAV_ITEMS: NavItem[] = [
  {
    id: "overview",
    label: "Overview",
    icon: "home",
    title: "Repository Overview",
    subtitle: "A quick look at what's happening in your repository.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "commits",
    label: "Commits",
    icon: "commit",
    title: "Commits",
    subtitle: "Browse the full history and inspect any commit.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "branches",
    label: "Branches",
    icon: "branch",
    title: "Branches",
    subtitle: "Create, switch, rename and delete branches — and merge or rebase them.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "stashes",
    label: "Stashes",
    icon: "stash",
    title: "Stashes",
    subtitle: "Work you set aside to come back to.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "pull-requests",
    label: "Pull Requests",
    icon: "pull-request",
    title: "Pull Requests",
    subtitle: "Open pull and merge requests for this repository's forge.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "issues",
    label: "Issues",
    icon: "issue",
    title: "Issues",
    subtitle: "Track work items alongside your code.",
    requiresRepository: false,
    available: false,
  },
  {
    id: "files",
    label: "Files",
    icon: "file",
    title: "Working Tree",
    subtitle: "Stage your changes, write a commit, or amend the last one.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "diff",
    label: "Diff",
    icon: "diff",
    title: "Diff",
    subtitle: "Compare staged and unstaged changes line by line.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "blame",
    label: "Blame",
    icon: "blame",
    title: "Blame",
    subtitle: "See who last touched each line of a file, and why.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "tags",
    label: "Tags",
    icon: "tag",
    title: "Tags",
    subtitle: "Named points in history — releases and milestones.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "remotes",
    label: "Remotes",
    icon: "remote",
    title: "Remotes",
    subtitle: "Where this repository fetches from and pushes to.",
    requiresRepository: true,
    available: true,
  },
  {
    id: "settings",
    label: "Settings",
    icon: "settings",
    title: "Settings",
    subtitle: "Repository, appearance, keyboard shortcuts and updates.",
    requiresRepository: false,
    available: true,
  },
];

export const DEFAULT_VIEW: ViewId = "overview";

export function navItem(id: ViewId): NavItem {
  const item = NAV_ITEMS.find((candidate) => candidate.id === id);
  if (!item) {
    throw new Error(`Unknown view id: ${id}`);
  }
  return item;
}

/**
 * Which view should actually render, given the one the person selected and
 * whether a repository is open.
 *
 * A repository-scoped view with no repository open falls back to
 * `settings` rather than to a dead empty screen, because Settings is where
 * the repository picker lives — so the fallback puts the person in front
 * of the control that fixes the situation. Views that do not require a
 * repository (Settings, Issues) are always honored as selected.
 */
export function resolveView(selected: ViewId, hasRepository: boolean): ViewId {
  const item = navItem(selected);
  if (item.requiresRepository && !hasRepository) {
    return "settings";
  }
  return selected;
}
