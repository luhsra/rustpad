import { Box, Flex, Portal, Text } from "@chakra-ui/react";
// import Editor from "@monaco-editor/react";
import { editor, languages } from "monaco-editor";
import * as monaco from 'monaco-editor';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';
import { useEffect, useRef, useState } from "react";
import useLocalStorageState from "use-local-storage-state";

import rustpadRaw from "../README.md?raw";
import Footer from "./Footer";
import Header from "./Header";
import animals from "./animals.json";
import { useColorMode } from "./color-mode";
import Rustpad, { type UserInfo } from "./rustpad";
import { Toaster, toaster } from "./toaster";
import useHash from "./useHash";
import { Editor, loader } from "@monaco-editor/react";

/// Configure web workers for vite
self.MonacoEnvironment = {
  getWorker(_, label) {
    if (label === 'json') {
      return new jsonWorker();
    }
    if (label === 'css' || label === 'scss' || label === 'less') {
      return new cssWorker();
    }
    if (label === 'html' || label === 'handlebars' || label === 'razor') {
      return new htmlWorker();
    }
    if (label === 'typescript' || label === 'javascript') {
      return new tsWorker();
    }
    return new editorWorker();
  },
};
/// Use bundle version of monaco editor
loader.config({ monaco });

export type ConnectionState = "connected" | "disconnected" | "desynchronized";

const version =
  typeof import.meta.env?.VITE_SHA === "string"
    ? import.meta.env.VITE_SHA.slice(0, 7)
    : "development";

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
        .removeEventListener("change", () => {});
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
        onChangeLanguage: (language) => {
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
    if (rustpad.current?.setLanguage(language)) {
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
        [{ range, text: rustpadRaw }],
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
        version={version}
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
