// The typed frontend service for forge (GitHub/GitLab) integration
// (EPIC-20; T-243/US-101, T-244/US-102). Following `sync.ts`'s convention:
// this is the only module that calls `invoke("get_forge_link" |
// "open_forge_link" | "forge_connection_status" | "connect_forge_account" |
// "disconnect_forge_account", ...)`. No forge/URL logic lives here — every
// call is a thin pass-through to a Tauri command that itself only calls
// `gitsail-application`/`gitsail-domain` (AGENTS.md).

import { invoke } from "@tauri-apps/api/core";

import type {
  ForgeAccountDto,
  ForgeConnectionStatusDto,
  ForgeLinkTargetDto,
} from "./dto";

/**
 * Resolves the browser URL `target` would open, or `null` when no
 * configured remote resolves to a known GitHub/GitLab forge (T-243/US-101
 * criterion 3 — never a rejected promise for this). Use this to decide
 * whether to show an "open in browser" action at all.
 */
export async function getForgeLink(target: ForgeLinkTargetDto): Promise<string | null> {
  return invoke<string | null>("get_forge_link", { target });
}

/**
 * Resolves `target` exactly like {@link getForgeLink} and, when a link is
 * found, launches it in the OS default browser. Resolves to `false` (never
 * a rejected promise) when no remote resolves to a known forge.
 */
export async function openForgeLink(target: ForgeLinkTargetDto): Promise<boolean> {
  return invoke<boolean>("open_forge_link", { target });
}

/** Whether `account` currently has a token connected (T-244/US-102
 * criterion 1). Never a rejected promise: a credential-store failure is
 * folded into `"notConnected"` server-side. */
export async function forgeConnectionStatus(
  account: ForgeAccountDto,
): Promise<ForgeConnectionStatusDto> {
  return invoke<ForgeConnectionStatusDto>("forge_connection_status", { account });
}

/** Connects `account`, storing `token` in OS-secure storage (T-244/US-102
 * criterion 2) — an explicit, user-initiated action. This never validates
 * `token` against the forge's live API (T-245, out of scope for US-102). */
export async function connectForgeAccount(account: ForgeAccountDto, token: string): Promise<void> {
  return invoke<void>("connect_forge_account", { account, token });
}

/** Disconnects `account`, removing its token from OS-secure storage
 * (T-244/US-102 criterion 2: a real deletion, not just clearing a cache). */
export async function disconnectForgeAccount(account: ForgeAccountDto): Promise<void> {
  return invoke<void>("disconnect_forge_account", { account });
}
