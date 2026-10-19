import { defineConfig } from "vite";

const base = process.env.GITHUB_PAGES === "true" ? "/jade-pdf/" : "/";

export default defineConfig({
  base,
});
