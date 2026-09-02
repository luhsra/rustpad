import { fromMarkdown } from "mdast-util-from-markdown";
import type { List, PhrasingContent, RootContent } from "mdast";
import Quill, { Delta, type Range } from "quill";

type Line = {
  content: PhrasingContent[] | string;
  formats: Record<string, boolean | number | string>;
};

export const headerAutofill = {
  key: " ",
  shiftKey: null,
  collapsed: true,
  format: {
    "code-block": false,
    blockquote: false,
    table: false,
  },
  prefix: /^#{1,6}$/,
  handler(this: { quill: Quill }, range: Range, context: { prefix: string }) {
    const level = context.prefix.length;
    const history = this.quill.getModule("history") as { cutoff: () => void };
    this.quill.insertText(range.index, " ", Quill.sources.USER);
    history.cutoff();
    this.quill.deleteText(range.index - level, level + 1, Quill.sources.USER);
    this.quill.formatLine(
      range.index - level,
      1,
      "header",
      level,
      Quill.sources.USER,
    );
    history.cutoff();
    this.quill.setSelection(range.index - level, Quill.sources.SILENT);
    return false;
  },
};

function markdownToDelta(markdown: string) {
  const lines: Line[] = [];
  let hasMarkdown = false;

  const addCodeBlock = (value: string) => {
    hasMarkdown = true;
    for (const line of value.split("\n")) {
      lines.push({ content: line, formats: { "code-block": true } });
    }
  };

  const addList = (list: List, indent = 0) => {
    hasMarkdown = true;
    const formats = {
      list: list.ordered ? "ordered" : "bullet",
      ...(indent > 0 ? { indent } : {}),
    };

    for (const item of list.children) {
      for (const child of item.children) {
        if (child.type === "paragraph") {
          lines.push({ content: child.children, formats });
        } else if (child.type === "list") {
          addList(child, indent + 1);
        } else if (child.type === "code") {
          addCodeBlock(child.value);
        }
      }
    }
  };

  let previousBlockEnd: number | undefined;
  for (const node of fromMarkdown(markdown).children) {
    const blockStart = node.position?.start.offset;
    if (previousBlockEnd !== undefined && blockStart !== undefined) {
      const newlines = markdown.slice(previousBlockEnd, blockStart).match(/\r?\n/g)?.length ?? 0;
      for (let index = 1; index < newlines; index += 1) {
        lines.push({ content: "", formats: {} });
      }
    }

    if (node.type === "heading") {
      lines.push({ content: node.children, formats: { header: node.depth } });
      hasMarkdown = true;
    } else if (node.type === "paragraph") {
      lines.push({ content: node.children, formats: {} });
    } else if (node.type === "list") {
      addList(node);
    } else if (node.type === "code") {
      addCodeBlock(node.value);
    }

    previousBlockEnd = node.position?.end.offset;
  }

  const delta = new Delta();
  const lineFormats: { index: number; formats: Line["formats"] }[] = [];

  const insertInline = (
    children: PhrasingContent[],
    formats: Record<string, boolean | number | string> = {},
  ) => {
    for (const child of children) {
      if (child.type === "text") {
        delta.insert(child.value.replaceAll("\n", " "), formats);
      } else if (child.type === "strong") {
        hasMarkdown = true;
        insertInline(child.children, { ...formats, bold: true });
      } else if (child.type === "emphasis") {
        hasMarkdown = true;
        insertInline(child.children, { ...formats, italic: true });
      } else if (child.type === "inlineCode") {
        delta.insert(child.value, formats);
      } else if (child.type === "link") {
        insertInline(child.children, { ...formats, link: child.url });
      } else if (child.type === "break") {
        delta.insert("\n", formats);
      }
    }
  };

  for (const [lineIndex, line] of lines.entries()) {
    const index = delta.length();
    if (typeof line.content === "string") {
      delta.insert(line.content);
    } else {
      insertInline(line.content);
    }
    if (Object.keys(line.formats).length > 0) lineFormats.push({ index, formats: line.formats });
    if (lineIndex < lines.length - 1) delta.insert("\n");
  }

  return { delta, hasMarkdown, lineFormats };
}

export function pasteMarkdown(event: ClipboardEvent, quill: Quill) {
  const text = event.clipboardData?.getData("text/plain");
  const range = quill.getSelection(true);
  if (!text || !range) return;

  const { delta, hasMarkdown, lineFormats } = markdownToDelta(text);
  if (!hasMarkdown) return;

  event.preventDefault();
  quill.updateContents(
    new Delta().retain(range.index).delete(range.length).concat(delta),
    Quill.sources.USER,
  );
  for (const line of lineFormats) {
    quill.formatLine(range.index + line.index, 1, line.formats, Quill.sources.USER);
  }
  quill.setSelection(range.index + delta.length(), Quill.sources.SILENT);
}
