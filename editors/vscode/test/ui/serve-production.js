/**
 * Serves the PRODUCTION webview bundle for Playwright E2E tests.
 *
 * Reads the actual built files (out/webview/index.html, styles.css, monitor.js),
 * assembles them into a test page with the vscode API shim, and serves
 * JS files separately to avoid inline script escaping issues.
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
const bundleJs = fs.readFileSync(jsPath, "utf8");
const shimJs = fs.readFileSync(shimPath, "utf8").replace(/__MONITOR_PORT__/g, monitorPort);

// Replace CSP to allow inline styles and scripts from our server
html = html.replace(
  /content="[^"]*"/,
  `content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline' http: ;"`
);

// Inline the CSS
html = html.replace(
  /<link rel="stylesheet" href="{{stylesUri}}">/,
  `<style>${css}</style>`
);

// Remove nonce attributes
html = html.replace(/nonce="{{nonce}}"/g, "");
html = html.replace(/{{cspSource}}/g, "'unsafe-inline'");

// Inject empty initial state
html = html.replace(
  "{{initialState}}",
  JSON.stringify({ catalog: [], watchList: [], expandedNodes: [], version: "test" })
);

// Replace script src with paths served by our HTTP server
html = html.replace(
  /<script[^>]*src="{{scriptUri}}"[^>]*><\/script>/,
  `<script src="/shim.js"></script>\n<script src="/monitor.js"></script>`
);

// Serve
const server = http.createServer((req, res) => {
  if (req.url === "/shim.js") {
    res.writeHead(200, { "Content-Type": "application/javascript; charset=utf-8" });
    res.end(shimJs);
  } else if (req.url === "/monitor.js") {
    res.writeHead(200, { "Content-Type": "application/javascript; charset=utf-8" });
    res.end(bundleJs);
  } else {
    res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
    res.end(html);
  }
});

server.listen(httpPort, () => {
  const addr = server.address();
  console.log(`Production webview served on http://localhost:${addr.port}`);
});
