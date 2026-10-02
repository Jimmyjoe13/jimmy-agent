import { defineConfig } from "vite";

// Tauri attend un port fixe en développement : la fenêtre est ouverte par
// l'application Rust, pas par Vite.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/godot/**"] },
  },
  build: {
    target: "chrome110",
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
  },
});