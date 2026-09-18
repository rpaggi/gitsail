import { describe, expect, it, vi } from "vitest";

import { TrustGate, WorkspaceTrustSource } from "../src/workspaceTrust";

function fakeSource(initiallyTrusted: boolean): WorkspaceTrustSource & { grant(): void } {
  let listener: (() => void) | undefined;
  return {
    isTrusted: initiallyTrusted,
    onDidGrantWorkspaceTrust(l) {
      listener = l;
      return { dispose: () => (listener = undefined) };
    },
    grant() {
      listener?.();
    },
  };
}

describe("TrustGate (T-204 criterion 2)", () => {
  it("reflects the initial trust state", () => {
    expect(new TrustGate(fakeSource(true)).isTrusted()).toBe(true);
    expect(new TrustGate(fakeSource(false)).isTrusted()).toBe(false);
  });

  it("becomes trusted once the workspace grants trust", () => {
    const source = fakeSource(false);
    const gate = new TrustGate(source);
    expect(gate.isTrusted()).toBe(false);

    source.grant();

    expect(gate.isTrusted()).toBe(true);
  });

  it("notifies subscribers exactly once per grant", () => {
    const source = fakeSource(false);
    const gate = new TrustGate(source);
    const listener = vi.fn();
    gate.onDidChange(listener);

    source.grant();

    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("never fires a change notification for the initial state", () => {
    const gate = new TrustGate(fakeSource(true));
    const listener = vi.fn();
    gate.onDidChange(listener);
    expect(listener).not.toHaveBeenCalled();
  });

  it("stops notifying a listener after its subscription is disposed", () => {
    const source = fakeSource(false);
    const gate = new TrustGate(source);
    const listener = vi.fn();
    const subscription = gate.onDidChange(listener);

    subscription.dispose();
    source.grant();

    expect(listener).not.toHaveBeenCalled();
  });

  it("dispose() releases the underlying subscription and clears listeners", () => {
    const disposeSpy = vi.fn();
    const gate = new TrustGate({
      isTrusted: false,
      onDidGrantWorkspaceTrust: () => ({ dispose: disposeSpy }),
    });

    gate.dispose();

    expect(disposeSpy).toHaveBeenCalledTimes(1);
  });
});
