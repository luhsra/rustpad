import useHash from "@/useHash";
import { Crepe } from "@milkdown/crepe";
import "@milkdown/crepe/theme/common/style.css";
import { editorViewOptionsCtx, rootDOMCtx } from "@milkdown/kit/core";
import { collab, collabServiceCtx } from "@milkdown/plugin-collab";
import { Milkdown, MilkdownProvider, useEditor } from "@milkdown/react";
import { type FC, useEffect, useState } from "react";
import { Awareness } from "y-protocols/awareness.js";
import { WebsocketProvider } from "y-websocket";
import { Doc } from "yjs";

import "./Editor.css";
import type { OnlineUser, UserRole } from "@/User";

function getWsUri() {
  let protocol = location.protocol == "https:" ? "wss:" : "ws:";
  return new URL(protocol + "//" + location.host + "/api/collab");
}

export type ConnectionStatus = "connected" | "disconnected" | "desynchronized";

export interface MilkdownEditorProps {
  dark?: boolean;
  name: string;
  color: string;
  role: UserRole;
  onConnectionChange?: (status: ConnectionStatus) => void;
  onConnectionError?: (error: Event) => void;
  onUserChange?: (users: Record<number, OnlineUser>) => void;
}

export const MilkdownEditor: FC<MilkdownEditorProps> = ({
  dark,
  name,
  color,
  role,
  onConnectionChange,
  onConnectionError,
  onUserChange,
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

        awareness = new Awareness(doc);
        awareness.setLocalStateField("user", { name, color, role });
        awareness.on("change", (change: any) => {
          console.info("Awareness change:", change, wsProvider.awareness.getStates());
          console.info("List:", Array.from(wsProvider.awareness.getStates().values()));

          const users: Record<number, OnlineUser> = {};
          for (const [id, state] of wsProvider.awareness.getStates().entries()) {
            const user: OnlineUser | undefined = state.user;
            if (user) {
              users[id] = {
                name: user.name,
                color: user.color,
                role: user.role ?? "anon",
              };
            }
          }
          console.info("Users:", users);
          onUserChange?.(users);
        });

        const wsProvider = new WebsocketProvider(
          getWsUri().toString(),
          id,
          doc,
          { connect: true, awareness },
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
        window.addEventListener("beforeunload", () => {
          wsProvider.destroy();
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
        awareness?.setLocalStateField("user", { name, color, role });
      });
    }
  }, [name, color, role]);

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
