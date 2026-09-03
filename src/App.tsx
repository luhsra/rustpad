import { Box, Flex, Portal, Text } from "@chakra-ui/react";
import Quill from "quill";
import { useEffect, useRef, useState } from "react";
import useLocalStorageState from "use-local-storage-state";

import Footer from "./Footer";
import Header from "./Header";
import animals from "./animals.json";
import Editor from "./components/Editor";
import { useColorMode } from "./components/color-mode";
import { Toaster, toaster } from "./components/toaster";
import Rustpad, {
  type OnlineUser,
  type UserRole,
  type Visibility,
} from "./rustpad";
import useHash from "./useHash";

export type ConnectionState = "connected" | "disconnected" | "desynchronized";

const VERSION = "dev";

function getWsUri(id: string) {
  const url = new URL(`api/socket/${id}`, window.location.href);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  return url.href;
}

function generateName() {
  return animals[Math.floor(Math.random() * animals.length)]!;
}

function generateHue() {
  return Math.floor(Math.random() * 360);
}

function App() {
  const [connection, setConnection] = useState<ConnectionState>("disconnected");
  const [users, setUsers] = useState<Record<number, OnlineUser>>({});
  const [name, setName] = useLocalStorageState("name", {
    defaultValue: generateName,
  });
  const [hue, setHue] = useLocalStorageState("hue", {
    defaultValue: generateHue,
  });
  const [role, setRole] = useState<UserRole>("anon");
  const editor = useRef<Quill>(null);
  const [visibility, setVisibility] = useState<Visibility>("public");
  const rustpad = useRef<Rustpad | undefined>(undefined);
  const { colorMode, setColorMode, toggleColorMode } = useColorMode();
  const id = useHash();

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const updateColorMode = (event: MediaQueryListEvent | MediaQueryList) =>
      setColorMode(event.matches ? "dark" : "light");
    updateColorMode(media);
    media.addEventListener("change", updateColorMode);
    return () => media.removeEventListener("change", updateColorMode);
  }, [setColorMode]);

  useEffect(() => {
    if (!editor.current) return;
    editor.current.setText("", "silent");
    const history = editor.current.getModule("history") as {
      clear: () => void;
    };
    history.clear();
    rustpad.current = new Rustpad({
      uri: getWsUri(id),
      editor: editor.current,
      onConnected: (info) => {
        if (info) {
          setName(info.name);
          setRole(info.role);
          setHue(info.hue);
        }
        setConnection("connected");
      },
      onDisconnected: () => setConnection("disconnected"),
      onDesynchronized: () => {
        setConnection("desynchronized");
        toaster.create({
          title: "Desynchronized with server",
          description: "Please save your work and refresh the page.",
          type: "error",
          duration: undefined,
          closable: true,
        });
      },
      onError: () => {
        setConnection("disconnected");
        toaster.create({
          title: "Cannot open document",
          description:
            "The name can only contain letters, numbers, hyphens and underscores.",
          type: "error",
          duration: undefined,
          closable: true,
        });
      },
      onChangeMeta: setVisibility,
      onChangeUsers: setUsers,
      onChangeMe: (info) => {
        setName(info.name);
        setRole(info.role);
        setHue(info.hue);
      },
    });
    return () => {
      rustpad.current?.dispose();
      rustpad.current = undefined;
    };
  }, [editor, id, setHue, setName]);

  useEffect(() => {
    if (connection === "connected")
      rustpad.current?.setInfo({ name, hue, role });
  }, [connection, hue, name, role]);

  function handleVisibilityChange(nextVisibility: Visibility) {
    setVisibility(nextVisibility);
    if (rustpad.current?.setVisibility(nextVisibility)) {
      toaster.create({
        title: "Visibility updated",
        description: (
          <>
            The document is now{" "}
            <Text as="span" fontWeight="semibold">
              {nextVisibility}
            </Text>
            .
          </>
        ),
        type: "info",
        duration: 2000,
        closable: true,
      });
    }
  }

  return (
    <Flex direction="column" h="100vh" overflow="hidden" data-theme={colorMode}>
      <Header
        toggleColorMode={toggleColorMode}
        version={VERSION}
        connection={connection}
      />
      <Box flex="1 1 auto" minH={0} className="editor-shell">
        <Editor ref={editor} />
      </Box>
      <Footer
        visibility={visibility}
        currentUser={{ name, hue, role }}
        users={users}
        onSetVisibility={handleVisibilityChange}
        onChangeName={(nextName) => nextName.length > 0 && setName(nextName)}
        onChangeColor={() => setHue(generateHue())}
      />
      <Portal>
        <Toaster />
      </Portal>
    </Flex>
  );
}

export default App;
