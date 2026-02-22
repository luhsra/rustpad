import useHash from "@/useHash";
import { Crepe } from "@milkdown/crepe";
import "@milkdown/crepe/theme/common/style.css";
import { editorViewOptionsCtx, rootDOMCtx } from "@milkdown/kit/core";
import { collab, collabServiceCtx } from "@milkdown/plugin-collab";
import { Milkdown, MilkdownProvider, useEditor } from "@milkdown/react";
import { type FC, useEffect, useState } from "react";
import type { Awareness } from "y-protocols/awareness.js";
import { WebsocketProvider } from "y-websocket";
import { Doc } from "yjs";

import "./Editor.css";

function getWsUri() {
  let protocol = location.protocol == "https:" ? "wss:" : "ws:";
  return new URL(protocol + "//" + location.host + "/api/collab");
}

export type ConnectionStatus = "connected" | "disconnected" | "desynchronized";

export interface MilkdownEditorProps {
  dark?: boolean;
  name: string;
  color: string;
  onConnectionChange?: (status: ConnectionStatus) => void;
  onConnectionError?: (error: Event) => void;
}

export const MilkdownEditor: FC<MilkdownEditorProps> = ({
  dark,
  name,
  color,
  onConnectionChange,
  onConnectionError,
}) => {
  const id = useHash();

  let awareness: Awareness | null = null;

  let { get, loading } = useEditor((root) => {
    // return editor;
    const editor = new Crepe({
      root,
      features: {
        [Crepe.Feature.Cursor]: false,
        [Crepe.Feature.Toolbar]: true,
        [Crepe.Feature.Latex]: true,
      },
    });
    editor.editor.use(collab);
    return editor;
  });

  useEffect(() => {
    if (!loading) {
      get()?.action((ctx) => {
        const wsUri = getWsUri();
        console.info("Connect:", wsUri.toString());
        const doc = new Doc();
        const wsProvider = new WebsocketProvider(
          getWsUri().toString(),
          id,
          doc,
          { connect: true },
        );
        wsProvider.on("connection-error", (event) => {
          console.error("WebSocket connection error:", event);
          onConnectionError?.(event);
        });
        wsProvider.on("connection-close", (event) => {
          console.warn("WebSocket connection closed:", event);
        });
        wsProvider.on("status", (event) => {
          onConnectionChange?.(
            event.status === "connected" ? "connected" : "disconnected",
          );
        });

        awareness = wsProvider.awareness;
        awareness.setLocalStateField("user", { name, color });
        awareness.on("change", () => {
          console.info("Awareness change:", wsProvider.awareness.getStates());
        });
        ctx
          .get(collabServiceCtx)
          .bindDoc(doc)
          .setAwareness(awareness)
          .connect();
      });
    }
  }, [id, loading]);

  useEffect(() => {
    if (!loading) {
      get()?.action((ctx) => {
        awareness?.setLocalStateField("user", { name, color });
      });
    }
  }, [name, color]);

  useEffect(() => {
    if (!loading) {
      console.info("Set theme:", dark ? "dark" : "light");
      get()?.action((ctx) => {
        ctx.get(rootDOMCtx).classList.toggle("dark", dark);
      });
    }
  }, [loading, dark]);

  return <Milkdown />;
};

export const MilkdownEditorWrapper: React.FC<MilkdownEditorProps> = (
  props: MilkdownEditorProps,
) => {
  return (
    <MilkdownProvider>
      <MilkdownEditor {...props} />
    </MilkdownProvider>
  );
};
