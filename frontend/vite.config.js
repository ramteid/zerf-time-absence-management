import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const frontendDebugBuild = process.env.KITAZEIT_FRONTEND_DEBUG_BUILD === "true";

export default defineConfig({
  plugins: [svelte()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    minify: frontendDebugBuild ? false : undefined,
    sourcemap: frontendDebugBuild,
    target: "es2020",
    rollupOptions: frontendDebugBuild
      ? {
          output: {
            entryFileNames: "assets/[name].js",
            chunkFileNames: "assets/[name].js",
          },
        }
      : undefined,
  },
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:3000",
      "/healthz": "http://127.0.0.1:3000",
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
    include: ["src/**/*.test.{js,svelte}"],
  },
});
