import { Box, Flex, Portal } from "@chakra-ui/react";
import { useEffect, useState } from "react";

import Footer from "./Footer";
import Header from "./Header";
import type { UserRole } from "./User";
import animals from "./animals.json";
import {
  type ConnectionStatus,
  MilkdownEditorWrapper,
} from "./components/Editor";
import { useColorMode } from "./components/color-mode";
import { Toaster, toaster } from "./components/toaster";
import { HSLToHex } from "./util";

const VERSION = "dev";

function generateColor() {
  const hue = Math.floor(Math.random() * 360);
  const rgb = HSLToHex({ h: hue, s: 100, l: 50 });
  return rgb;
}

function NewApp() {
  const { colorMode, setColorMode, toggleColorMode } = useColorMode();

  const [color, setColor] = useState(generateColor());
  const [name, setName] = useState(
    animals[Math.floor(Math.random() * animals.length)]!,
  );
  const [role, setRole] = useState<UserRole>("anon");

  const [connection, setConnection] =
    useState<ConnectionStatus>("disconnected");

  useEffect(() => {
    setColorMode(
      window.matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light",
    );
    // Add listener to update styles
    window
      .matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", (e) =>
        setColorMode(e.matches ? "dark" : "light"),
      );
    // Remove listener
    return () => {
      window
        .matchMedia("(prefers-color-scheme: dark)")
        .removeEventListener("change", () => {});
    };
  }, []);

  return (
    <Flex direction="column" h="100vh" overflow="hidden">
      <Header
        toggleColorMode={toggleColorMode}
        version={VERSION}
        connection={connection}
      />
      <Box flex="1 0" minH={0}>
        <MilkdownEditorWrapper
          dark={colorMode === "dark"}
          name={name}
          color={color}
          onConnectionChange={setConnection}
          onConnectionError={(error) =>
            toaster.error({
              title: "Connection error",
              description: "" + error,
              closable: true,
            })
          }
        />
      </Box>
      <Footer
        visibility={"public"}
        currentUser={{ name, color, role }}
        users={[]}
        onSetVisibility={() => {}}
        onLoadSample={() => {}}
        onChangeName={(name) => name.length > 0 && setName(name)}
        onChangeColor={() => setColor(generateColor())}
      />
      <Portal>
        <Toaster />
      </Portal>
    </Flex>
  );
}

export default NewApp;
