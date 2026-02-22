import { MilkdownProvider } from "@milkdown/react";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { ChakraProvider, defaultSystem } from "@chakra-ui/react";
import { ColorModeProvider } from "./components/color-mode";

import App from "./App";

const root$ = document.getElementById("root");
if (!root$) throw new Error("No root element found");

const root = createRoot(root$);

root.render(
  <StrictMode>
    <ChakraProvider value={defaultSystem}>
      <ColorModeProvider>
        <App />
      </ColorModeProvider>
    </ChakraProvider>
  </StrictMode>,
);
