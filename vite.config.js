import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Cargo frequently locks .pdb/.exe files while compiling on Windows.
      // They are not frontend inputs, so Vite should never watch them.
      ignored: /src-tauri[\\/]target[\\/]/
    }
  }
});
