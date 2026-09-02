import { serve } from "bun";

import index from "./index.html";

const PROXY_TARGET = "localhost:3030";
const HTTP_TARGET = `http://${PROXY_TARGET}`;
const WS_TARGET = `ws://${PROXY_TARGET}`;

const server = serve({
  routes: {
    "/": index,
    "/api/*": (request) => {
      const url = new URL(request.url);
      if (request.headers.get("upgrade")?.toLowerCase() === "websocket") {
        server.upgrade(request, { data: { url, backend: undefined } });
        return undefined;
      }
      return fetch(new URL(url.pathname, HTTP_TARGET), {
        method: request.method,
        headers: request.headers,
        body: request.body,
      });
    },
  },
  websocket: {
    open(socket) {
      if (socket.data.backend) return;
      const backend = new WebSocket(
        new URL(socket.data.url.pathname, WS_TARGET),
      );
      socket.data.backend = backend;
      backend.onmessage = (event) => socket.send(event.data);
      backend.onclose = () => socket.close();
      backend.onerror = () => socket.close();
    },
    message(socket, message) {
      socket.data.backend?.send(message);
    },
    close(socket) {
      socket.data.backend?.close();
    },
  } as Bun.WebSocketHandler<{ url: URL; backend?: WebSocket }>,
  development: true,
});

console.log(`Listening on ${server.url}`);
