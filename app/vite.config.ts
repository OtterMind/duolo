import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri 开发模式固定连 1420 端口。
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  // 本地应用，不需要按体积拆包。
  build: { target: "safari15", outDir: "dist", emptyOutDir: true, chunkSizeWarningLimit: 2000 },
});
