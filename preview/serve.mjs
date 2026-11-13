// Live-reloading dev server for the preview page. No dependencies.
//
// Design borrowed from automerge.github.io's `system/server.ts` (itself from
// ivyreese/please-reload): serve files with caching off, inject a script into
// HTML that reloads on a signal, and watch sources. Differences:
//
//   - Server-Sent Events instead of WebSockets: the signal is one-way, and
//     Node has no built-in WebSocket server.
//   - Two watch sets. Rust sources trigger a Wasm rebuild and then a reload;
//     preview/ and packages/ files only trigger a reload. Build output
//     (packages/rain-diagram/pkg/) is never watched, so a build can't
//     trigger another build.
//   - /packages/… is served from packages/, so the preview runs the same
//     component and Wasm the npm package ships.
//   - Like their infinite-reload fix, a "change" only counts if the file's
//     mtime actually moved. Some platforms (Linux inotify) report
//     metadata-only changes too.
//
// Usage: node preview/serve.mjs   (env: PORT, default 8080)

import { spawn } from "node:child_process"
import * as FS from "node:fs"
import * as HTTP from "node:http"
import * as Path from "node:path"

const previewDir = Path.dirname(new URL(import.meta.url).pathname)
const root = Path.dirname(previewDir)
const packagesDir = Path.join(root, "packages")
const pkgDir = Path.join(packagesDir, "rain-diagram", "pkg")

const MIME = {
  css: "text/css",
  html: "text/html",
  js: "text/javascript",
  json: "application/json",
  mjs: "text/javascript",
  rain: "text/plain; charset=utf-8",
  svg: "image/svg+xml",
  ts: "text/plain",
  wasm: "application/wasm",
}

const RELOAD_SCRIPT = `<script>
  const events = new EventSource("/__events")
  events.addEventListener("reload", () => location.reload())
  events.addEventListener("build-error", (e) => window.dispatchEvent(new CustomEvent("rain:build-error", { detail: JSON.parse(e.data) })))
</script>`

const log = (msg) => console.log(`\x1b[2m${new Date().toLocaleTimeString()}\x1b[0m ${msg}`)

// --- Events ----------------------------------------------------------------

const clients = new Set()

const broadcast = (event, data = "") => {
  for (const res of clients) res.write(`event: ${event}\ndata: ${JSON.stringify(data)}\n\n`)
}

// --- Build -----------------------------------------------------------------

let building = false
let pending = false

const build = () => {
  if (building) {
    pending = true
    return
  }

  building = true
  log("building Wasm…")

  const args = ["build", "--dev", "--target", "web", "--out-dir", pkgDir, Path.join(root, "rain_diagram_wasm")]
  const child = spawn("wasm-pack", args, { cwd: root })
  let output = ""
  child.stdout.on("data", (d) => (output += d))
  child.stderr.on("data", (d) => (output += d))

  child.on("close", (code) => {
    building = false

    if (code === 0) {
      log("\x1b[32mbuilt\x1b[0m, reloading")
      broadcast("reload")
    } else {
      log(`\x1b[31mbuild failed\x1b[0m\n${output}`)
      broadcast("build-error", output.replace(/\x1b\[[0-9;]*m/g, ""))
    }

    if (pending) {
      pending = false
      build()
    }
  })
}

// --- Watch -----------------------------------------------------------------

const debounce = (ms, f) => {
  let timer
  return () => {
    clearTimeout(timer)
    timer = setTimeout(f, ms)
  }
}

const mtimes = new Map()

// Only count a change if the file's content timestamp moved
const reallyChanged = (path) => {
  try {
    const mtime = FS.statSync(path).mtimeMs
    if (mtimes.get(path) === mtime) return false
    mtimes.set(path, mtime)
    return true
  } catch {
    return mtimes.delete(path) // deleted: counts once
  }
}

const watch = (dir, { ignore = () => false, onChange }) => {
  // Record starting mtimes so the first metadata-only event isn't mistaken
  // for an edit
  for (const file of FS.readdirSync(dir, { recursive: true })) {
    if (!ignore(file)) reallyChanged(Path.join(dir, file))
  }

  const run = debounce(150, onChange)
  FS.watch(dir, { recursive: true }, (_event, file) => {
    if (!file || ignore(file)) return
    if (reallyChanged(Path.join(dir, file))) run()
  })
}

const rustDirs = ["rain_diagram_core", "rain_diagram_wasm"].map((crate) => Path.join(root, crate))
// Skip editor lock/backup files such as Emacs's `.#foo.rs` and `#foo.rs#`
const isRustSource = (file) => {
  const base = Path.basename(file)
  return !/^[.#]/.test(base) && (base.endsWith(".rs") || base === "Cargo.toml")
}

for (const dir of rustDirs) {
  watch(dir, { ignore: (file) => !isRustSource(file), onChange: build })
}

const reload = () => {
  log("preview changed, reloading")
  broadcast("reload")
}
watch(previewDir, {
  ignore: (file) => file === "serve.mjs" || file.startsWith("."),
  onChange: reload,
})
watch(packagesDir, {
  ignore: (file) => file.split(Path.sep).some((part) => part === "pkg" || part === "node_modules" || part.startsWith(".")),
  onChange: reload,
})

// --- Serve -----------------------------------------------------------------

const NO_CACHE = {
  "Cache-Control": "no-cache, no-store, must-revalidate",
  Expires: "-1",
  Pragma: "no-cache",
}

const server = HTTP.createServer((req, res) => {
  const url = decodeURI((req.url ?? "/").split("?")[0])

  if (url === "/__events") {
    res.writeHead(200, { ...NO_CACHE, "Content-Type": "text/event-stream", Connection: "keep-alive" })
    res.write(": connected\n\n")
    clients.add(res)
    req.on("close", () => clients.delete(res))
    return
  }

  // /packages/… from packages/, everything else from preview/
  const packaged = url.startsWith("/packages/")
  const base = packaged ? packagesDir : previewDir
  const path = packaged ? url.slice("/packages".length) : url
  const file = Path.join(base, path.endsWith("/") ? path + "index.html" : path)
  if (!file.startsWith(base + Path.sep)) {
    res.writeHead(403).end()
    return
  }

  const type = MIME[Path.extname(file).slice(1)]
  if (!type) {
    res.writeHead(415).end()
    return
  }

  FS.readFile(file, (error, content) => {
    if (error) {
      res.writeHead(error.code === "ENOENT" ? 404 : 500).end()
      return
    }

    if (type === "text/html") {
      const html = content.toString()
      content = html.includes("</body>") ? html.replace("</body>", `${RELOAD_SCRIPT}\n</body>`) : html + RELOAD_SCRIPT
    }

    res.writeHead(200, { ...NO_CACHE, "Content-Type": type }).end(content)
  })
})

let port = Number(process.env.PORT ?? 8080)

server.on("error", (err) => {
  if (err.code !== "EADDRINUSE") throw err
  log(`port ${port} in use, trying ${port + 1}`)
  server.listen(++port, "127.0.0.1")
})

server.on("listening", () => log(`preview at \x1b[32mhttp://127.0.0.1:${port}/\x1b[0m`))

build()
server.listen(port, "127.0.0.1")
