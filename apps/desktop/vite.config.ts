import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Tauri-specific settings (https://v2.tauri.app/start/frontend/vite/):
// fixed dev server port so `tauri.conf.json`'s `devUrl` stays valid, and
// ignore `src-tauri` so Rust rebuilds never trigger a Vite reload.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
