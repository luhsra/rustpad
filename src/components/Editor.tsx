import { headerAutofill, pasteMarkdown } from "@/markdown";
import Quill, { Delta, Range } from "quill";
import "quill/dist/quill.bubble.css";
import React from "react";

interface EditorProps {
  readOnly?: boolean;
  defaultValue?: Delta;
  onTextChange?: (delta: Delta, oldDelta: Delta, source: string) => void;
  onSelectionChange?: (
    range: Range | null,
    oldRange: Range | null,
    source: string,
  ) => void;
}

type TextChangeHandler = NonNullable<EditorProps["onTextChange"]>;
type SelectionChangeHandler = NonNullable<EditorProps["onSelectionChange"]>;

function setRef<T>(ref: React.ForwardedRef<T>, value: T | null) {
  if (typeof ref === "function") {
    ref(value);
  } else if (ref) {
    ref.current = value;
  }
}

// Editor is an uncontrolled React component.
const Editor = React.forwardRef<Quill, EditorProps>(
  (
    { readOnly = false, defaultValue, onSelectionChange, onTextChange },
    ref,
  ) => {
    const containerRef = React.useRef<HTMLDivElement>(null);
    const quillRef = React.useRef<Quill | null>(null);
    const defaultValueRef = React.useRef(defaultValue);
    const onTextChangeRef = React.useRef(onTextChange);
    const onSelectionChangeRef = React.useRef(onSelectionChange);

    React.useLayoutEffect(() => {
      onTextChangeRef.current = onTextChange;
      onSelectionChangeRef.current = onSelectionChange;
    }, [onSelectionChange, onTextChange]);

    React.useEffect(() => {
      quillRef.current?.enable(!readOnly);
    }, [readOnly]);

    React.useEffect(() => {
      const container = containerRef.current;
      if (!container) {
        return;
      }

      const quill = new Quill(container, {
        theme: "bubble",
        modules: {
          history: { userOnly: true },
          keyboard: {
            bindings: {
              "header autofill": headerAutofill,
            },
          },
          toolbar: [
            ["bold", "italic", "code", "link"],
            ["image", "code-block"],
            [{ header: 1 }, { header: 2 }, { list: "bullet" }],
          ],
        },
        formats: [
          "header",
          "bold",
          "italic",
          "code",
          "link",
          "image",
          "code-block",
          "list",
        ],
      });
      quillRef.current = quill;
      setRef(ref, quill);
      quill.enable(!readOnly);

      if (defaultValueRef.current) {
        quill.setContents(defaultValueRef.current);
      }

      const handleTextChange = (...args: Parameters<TextChangeHandler>) => {
        onTextChangeRef.current?.(...args);
      };
      const handleSelectionChange = (
        ...args: Parameters<SelectionChangeHandler>
      ) => {
        onSelectionChangeRef.current?.(...args);
      };

      quill.on(Quill.events.TEXT_CHANGE, handleTextChange);
      quill.on(Quill.events.SELECTION_CHANGE, handleSelectionChange);

      const handleMarkdownPaste = (event: ClipboardEvent) =>
        pasteMarkdown(event, quill);
      quill.root.addEventListener("paste", handleMarkdownPaste, true);

      return () => {
        quill.root.removeEventListener("paste", handleMarkdownPaste, true);
        quill.off(Quill.events.TEXT_CHANGE, handleTextChange);
        quill.off(Quill.events.SELECTION_CHANGE, handleSelectionChange);
        quillRef.current = null;
        setRef(ref, null);
        container.replaceChildren();
        container.removeAttribute("class");
      };
    }, [ref]);

    return <div ref={containerRef} />;
  },
);

Editor.displayName = "Editor";

export default Editor;
