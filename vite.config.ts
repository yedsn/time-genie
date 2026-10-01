import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const pkgVersion = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf-8")
).version as string;

export default defineConfig({
  plugins: [vue()],
  define: {
    __APP_VERSION__: JSON.stringify(pkgVersion)
  },
  root: "src-ui",
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 5297,
    strictPort: true
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    target: "es2020"
  }
});
