import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/** Packaged WebView drops stylesheets requested with the crossorigin attribute. */
function packagedStylesheet() {
  return {
    name: "rcodex-packaged-stylesheet",
    apply: "build" as const,
    transformIndexHtml(html: string) {
      return html.replaceAll(
        "<link rel=\"stylesheet\" crossorigin href=",
        "<link rel=\"stylesheet\" href=",
      );
    },
  };
}

export default defineConfig({
  plugins: [react(), packagedStylesheet()],
  // Packaged macOS WebView loads CSS from relative asset URLs, not /assets/...
  base: "./",
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true
  }
});
