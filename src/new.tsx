import { MilkdownProvider } from "@milkdown/react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { MilkdownEditor } from "./components/Editor";

const root$ = document.getElementById("root");
if (!root$) throw new Error("No root element found");

const root = createRoot(root$);

root.render(
    <StrictMode>
        <MilkdownProvider>
            <MilkdownEditor />
        </MilkdownProvider>
    </StrictMode>
);
