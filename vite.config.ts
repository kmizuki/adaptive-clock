import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

const devHost = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [
    svelte({
      compilerOptions: {
        css: "injected",
      },
    }),
  ],
  clearScreen: false,
  server: {
    host: devHost ?? false,
    port: 1420,
    strictPort: true,
    hmr: devHost
      ? {
          protocol: "ws",
          host: devHost,
          port: 1421,
        }
      : undefined,
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: ["es2021", "chrome100", "safari13"],
    minify: process.env.TAURI_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_DEBUG,
  },
});
