import { Box, Flex, Portal, Text } from "@chakra-ui/react";
import { editor, languages } from "monaco-editor";
import { useEffect, useRef, useState } from "react";
import useLocalStorageState from "use-local-storage-state";

// Configure monaco before using it
import "./monaco-config";

import readme from "../README.md";
import Footer from "./Footer";
import Header from "./Header";
import animals from "./animals.json";
import { useColorMode } from "./components/color-mode";
import Rustpad, { type UserInfo } from "./rustpad";
import { Toaster, toaster } from "./components/toaster";
import useHash from "./useHash";
import { Editor } from "@monaco-editor/react";

export type ConnectionState = "connected" | "disconnected" | "desynchronized";

const sampleText = typeof Bun !== "undefined"
  ? await Bun.file(readme as any).text()
  : await fetch(readme as any).then((response) => response.text());

const VERSION = "dev";

function getWsUri(id: string) {
  let url = new URL(`api/socket/${id}`, window.location.href);
  url.protocol = url.protocol == "https:" ? "wss:" : "ws:";
  return url.href;
}

function generateName() {
  return "Anonymous " + animals[Math.floor(Math.random() * animals.length)];
}

function generateHue() {
  return Math.floor(Math.random() * 360);
}

function App() {
  const [language, setLanguage] = useState("markdown");
  const [connection, setConnection] = useState<ConnectionState>("disconnected");
  const [users, setUsers] = useState<Record<number, UserInfo>>({});
  const [name, setName] = useLocalStorageState("name", {
    defaultValue: generateName,
  });
  const [hue, setHue] = useLocalStorageState("hue", {
    defaultValue: generateHue,
  });
  const [editor, setEditor] = useState<editor.IStandaloneCodeEditor>();
  const { colorMode, setColorMode, toggleColorMode } = useColorMode();
  const rustpad = useRef<Rustpad | undefined>(undefined);
  const id = useHash();

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
        .removeEventListener("change", () => { });
    };
  }, []);

  useEffect(() => {
    if (editor?.getModel()) {
      const model = editor.getModel()!;
      model.setValue("");
      model.setEOL(0); // LF
      rustpad.current = new Rustpad({
        uri: getWsUri(id),
        editor,
        onConnected: () => setConnection("connected"),
        onDisconnected: () => setConnection("disconnected"),
        onDesynchronized: () => {
          setConnection("desynchronized");
          toaster.create({
            title: "Desynchronized with server",
            description: "Please save your work and refresh the page.",
            type: "error",
            duration: undefined,
          });
        },
        onError: (error) => {
          setConnection("disconnected");
          toaster.create({
            title: "Cannot open document",
            description: "The name can only contain letters, numbers, hyphens and underscores.",
            type: "error",
            duration: undefined,
          });
        },
        onChangeMeta: (language, open) => {
          if (languages.getLanguages().some((it) => it.id === language)) {
            setLanguage(language);
          }
        },
        onChangeUsers: setUsers,
      });
      return () => {
        rustpad.current?.dispose();
        rustpad.current = undefined;
      };
    }
  }, [id, editor, toaster, setUsers]);

  useEffect(() => {
    if (connection === "connected") {
      rustpad.current?.setInfo({ name, hue });
    }
  }, [connection, name, hue]);

  function handleLanguageChange(language: string) {
    setLanguage(language);
    if (rustpad.current?.setMeta(language)) {
      toaster.create({
        title: "Language updated",
        description: (
          <>
            All users are now editing in{" "}
            <Text as="span" fontWeight="semibold">
              {language}
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

  function handleLoadSample() {
    if (editor?.getModel()) {
      const model = editor.getModel()!;
      const range = model.getFullModelRange();

      model.pushEditOperations(
        editor.getSelections(),
        [{ range, text: sampleText }],
        () => null,
      );
      editor.setPosition({ column: 0, lineNumber: 0 });
      if (language !== "markdown") {
        handleLanguageChange("markdown");
      }
    }
  }

  return (
    <Flex direction="column" h="100vh" overflow="hidden">
      <Header
        toggleColorMode={toggleColorMode}
        version={VERSION}
        connection={connection}
      />
      <Box flex="1 0" minH={0}>
        <Editor
          theme={colorMode === "dark" ? "vs-dark" : "vs"}
          language={language}
          options={{
            automaticLayout: true,
            fontSize: 13,
            minimap: { enabled: false },
          }}
          onMount={(editor) => setEditor(editor)}
        />
      </Box>
      <Footer
        language={language}
        currentUser={{ name, hue }}
        users={users}
        onLanguageChange={handleLanguageChange}
        onLoadSample={handleLoadSample}
        onChangeName={(name) => name.length > 0 && setName(name)}
        onChangeColor={() => setHue(generateHue())}
      />
      <Portal>
        <Toaster />
      </Portal>
    </Flex>
  );
}

export default App;
