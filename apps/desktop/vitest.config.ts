import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";

// jsdom: `@tauri-apps/api/mocks` (used by src/services/repository.test.ts)
// reads/writes `window`, which the default `node` Vitest environment does
// not provide.
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
  },
});
