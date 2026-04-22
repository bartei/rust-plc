import * as esbuild from "esbuild";
import { copyFileSync, mkdirSync, existsSync } from "fs";
import { resolve, dirname } from "path";

const watch = process.argv.includes("--watch");
const outDir = "out/webview";

// Ensure output directory exists
if (!existsSync(outDir)) {
  mkdirSync(outDir, { recursive: true });
}

// Copy static assets to out/webview/
copyFileSync("src/webview/index.html", `${outDir}/index.html`);
copyFileSync("src/webview/styles.css", `${outDir}/styles.css`);

/** @type {esbuild.BuildOptions} */
const buildOptions = {
  entryPoints: ["src/webview/index.tsx"],
  bundle: true,
  outfile: `${outDir}/monitor.js`,
  format: "iife",
  target: "es2022",
  sourcemap: true,
  minify: !watch,
  jsx: "automatic",
  jsxImportSource: "preact",
};

if (watch) {
  const ctx = await esbuild.context(buildOptions);
  await ctx.watch();
  console.log("Watching webview source...");
} else {
  await esbuild.build(buildOptions);
  console.log(`Built ${outDir}/monitor.js`);
}
