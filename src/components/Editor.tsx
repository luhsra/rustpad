import type { FC } from "react";

// import { Crepe } from "@milkdown/crepe";
import { Milkdown, useEditor } from "@milkdown/react";
import {
    collab,
    CollabService,
    collabServiceCtx,
} from '@milkdown/plugin-collab';
import { Editor, rootCtx } from "@milkdown/kit/core";
import { nord } from '@milkdown/theme-nord';
import { commonmark, syncHeadingIdPlugin } from "@milkdown/kit/preset/commonmark";
import { Doc } from "yjs";
import { WebsocketProvider } from "y-websocket";

// import "@milkdown/crepe/theme/common/style.css";
// import "@milkdown/crepe/theme/nord.css";

function getWsUri() {
    let protocol = location.protocol == "https:" ? "wss:" : "ws:";
    return new URL(protocol + "//" + location.host + "/api/collab/");
}

export const MilkdownEditor: FC = () => {
    useEditor((root) => {

        const editor = Editor.make()
            .config(nord)
            .config((ctx) => {
                ctx.set(rootCtx, root);
            })
            .use(commonmark)
            .use(collab);

        const doc = new Doc();
        const wsProvider = new WebsocketProvider(
            getWsUri().href, window.location.hash.substring(1), doc, { connect: true }
        );

        // To fix CJK issue
        editor.remove(syncHeadingIdPlugin);

        editor.action((ctx) => {
            const collabService = ctx.get(collabServiceCtx);

            collabService
                // bind doc and awareness
                .bindDoc(doc)
                .setAwareness(wsProvider.awareness)
                // connect yjs with milkdown
                .connect();
        });

        return editor;

    }, []);

    return <Milkdown />;
};
