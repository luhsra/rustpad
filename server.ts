import { serve } from "bun";
import index from "./index.html";
import newApp from "./new.html";

const PROXY_TARGET = "localhost:3030";
const HTTP_TARGET = "http://" + PROXY_TARGET;
const WS_TARGET = "ws://" + PROXY_TARGET;

const { outputs: [
    editorWorker,
    cssWorker,
    htmlWorker,
    jsonWorker,
    tsWorker,
] } = await Bun.build({
    entrypoints: [
        "./src/monaco/editor.worker.ts",
        "./src/monaco/css.worker.ts",
        "./src/monaco/html.worker.ts",
        "./src/monaco/json.worker.ts",
        "./src/monaco/ts.worker.ts",
    ],
    target: "browser",
});
function worker(lib: Bun.BuildArtifact): Response {
    return new Response(lib, { headers: { "Content-Type": "application/javascript" } });
}
console.info("worker", editorWorker)

const server = serve({
    routes: {
        "/": index,
        "/new": newApp,
        "/api/*": (req) => {
            const url = new URL(req.url);

            // Check for WebSocket upgrade
            if (req.headers.get('upgrade')?.toLowerCase() === 'websocket') {
                // Pass the URL and other request info to the WebSocket handler
                server.upgrade(req, { data: { url: url, backend: undefined } });
                return undefined; // Must return undefined after successful upgrade
            }

            const backendUrl = new URL(url.pathname, HTTP_TARGET);
            return fetch(backendUrl, {
                method: req.method,
                headers: req.headers,
                body: req.body,
            });
        },
        "/src/monaco/editor.worker.js": worker(editorWorker!),
        "/src/monaco/css.worker.js": worker(cssWorker!),
        "/src/monaco/html.worker.js": worker(htmlWorker!),
        "/src/monaco/json.worker.js": worker(jsonWorker!),
        "/src/monaco/ts.worker.js": worker(tsWorker!),
    },
    websocket: {
        open(ws) {
            if (ws.data.backend) {
                console.warn("WebSocket already has a backend connection");
                return;
            }
            // Access the URL from ws.data
            console.log("WebSocket opened for:", ws.data.url.toString());

            // Now you can use it to construct the backend WebSocket URL
            const path = ws.data.url.pathname;
            const backendUrl = new URL(path, WS_TARGET);
            const backend = new WebSocket(backendUrl);

            ws.data.backend = backend;
            backend.onopen = () => console.log('Backend WS connected');
            backend.onmessage = (event) => {
                console.log("recv:", event.data);
                ws.send(event.data)
            };
            backend.onclose = () => ws.close();
            backend.onerror = (err) => {
                console.error('Backend WS error:', err);
                ws.close();
            };
        },
        message(ws, message) {
            console.log("send:", message);
            ws.data.backend?.send(message);
        },
        close(ws) {
            console.log("WebSocket closed");
            ws.data.backend?.close();
        },
    } as Bun.WebSocketHandler<{ url: URL; backend: WebSocket | undefined }>,
    development: true,
});

console.log(`Listening on ${server.url}`);
