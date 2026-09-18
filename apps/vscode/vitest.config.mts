import { defineConfig } from "vitest/config";

// `node` environment: none of this extension's business logic touches the
// DOM (unlike `apps/desktop`, which needs `jsdom` for `@tauri-apps/api`'s
// mocks) — every module under test here is driven through the
// `ExtensionHost` interface (`src/hostTypes.ts`) with plain object mocks.
export default defineConfig({
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
