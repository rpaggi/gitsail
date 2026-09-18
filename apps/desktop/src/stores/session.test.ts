import { describe, expect, it, beforeEach } from "vitest";
import { createPinia, setActivePinia } from "pinia";

import { useRepositorySessionStore } from "./session";

describe("repository session store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("starts with no repository or status open", () => {
    const store = useRepositorySessionStore();

    expect(store.repository).toBeNull();
    expect(store.status).toBeNull();
  });

  it("only ever holds the fields SAD §21 assigns to a repository session", () => {
    const store = useRepositorySessionStore();

    expect(Object.keys(store.$state)).toEqual(["repository", "status"]);
  });
});
