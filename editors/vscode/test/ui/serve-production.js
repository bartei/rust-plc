/**
 * Serves the PRODUCTION webview bundle for Playwright E2E tests.
 *
 * Reads the actual built files (out/webview/index.html, styles.css, monitor.js),
 * inlines them into a single page, and prepends the vscode API shim that
 * bridges to the real Rust monitor WS server.
 *
 * Usage: node serve-production.js <monitor-ws-port> [http-port]
 */

const fs = require("fs");
const http = require("http");
const path = require("path");

const monitorPort = process.argv[2] || "0";
const httpPort = parseInt(process.argv[3] || "0", 10);
const outDir = path.resolve(__dirname, "..", "..", "out", "webview");

// Read production files
const htmlPath = path.join(outDir, "index.html");
const cssPath = path.join(outDir, "styles.css");
const jsPath = path.join(outDir, "monitor.js");
const shimPath = path.join(__dirname, "vscode-api-shim.js");

if (!fs.existsSync(htmlPath)) {
  console.error(`ERROR: ${htmlPath} not found. Run 'npm run build:webview' first.`);
  process.exit(1);
}

let html = fs.readFileSync(htmlPath, "utf8");
const css = fs.readFileSync(cssPath, "utf8");
const js = fs.readFileSync(jsPath, "utf8");
const shim = fs.readFileSync(shimPath, "utf8").replace(/__MONITOR_PORT__/g, monitorPort);

// Replace CSP to allow inline scripts (needed for the shim)
html = html.replace(
  /content="[^"]*"/,
  `content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline';"`
);

// Inline the CSS (replace the external stylesheet link)
html = html.replace(
  /<link rel="stylesheet" href="{{stylesUri}}">/,
  `<style>${css}</style>`
);

// Remove nonce attributes (not needed in test)
html = html.replace(/nonce="{{nonce}}"/g, "");

// Replace CSP source placeholder
html = html.replace(/{{cspSource}}/g, "'unsafe-inline'");

// Inject empty initial state
html = html.replace(
  "{{initialState}}",
  JSON.stringify({ catalog: [], watchList: [], expandedNodes: [] })
);

// Replace the external script reference with inline shim + production bundle
html = html.replace(
  /<script[^>]*src="{{scriptUri}}"[^>]*><\/script>/,
  `<script>${shim}\n${js}</script>`
);

// Serve
const server = http.createServer((req, res) => {
  res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
  res.end(html);
});

server.listen(httpPort, () => {
  const addr = server.address();
  console.log(`Production webview served on http://localhost:${addr.port}`);
});
