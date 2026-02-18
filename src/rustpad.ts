import debounce from "lodash.debounce";
import type { IDisposable, IPosition, editor } from "monaco-editor";
import { OpSeq, initSync } from "rustpad-wasm/rustpad_wasm";

// Bun cannot automatically bundle and init wasm modules, so we have to do it manually.
// See: https://github.com/flowscripter/template-bun-wasm-rust-library
import wasm from "rustpad-wasm/pkg/rustpad_wasm_bg.wasm";
const wasmBuffer = typeof Bun !== "undefined"
  ? await Bun.file(wasm as any).arrayBuffer()
  : await fetch(wasm as any).then((response) => response.arrayBuffer());


/** Options passed in to the Rustpad constructor. */
export type RustpadOptions = {
  readonly uri: string;
  readonly editor: editor.IStandaloneCodeEditor;
  readonly onConnected?: (info?: OnlineUser) => void;
  readonly onDisconnected?: () => void;
  readonly onDesynchronized?: () => void;
  readonly onError?: (error: Event) => void;
  readonly onChangeMeta?: (language: string, visibility: Visibility) => void;
  readonly onChangeUsers?: (users: Record<number, OnlineUser>) => void;
  readonly onChangeMe?: (info: OnlineUser) => void;
  readonly reconnectInterval?: number;
};

export type UserRole = "admin" | "user" | "anon";
export type Visibility = "public" | "internal" | "private";

export function canAccess(role: UserRole, visibility: Visibility): boolean {
  if (visibility === "public") {
    return true;
  } else if (visibility === "internal") {
    return role !== "anon";
  } else {
    return role === "admin";
  }
}

/** A user currently editing the document. */
export type OnlineUser = {
  readonly name: string;
  readonly hue: number;
  readonly role: UserRole;
};

/** Browser client for Rustpad. */
class Rustpad {
  private ws?: WebSocket;
  private connecting?: boolean;
  private recentFailures: number = 0;
  private readonly model: editor.ITextModel;
  private readonly onChangeHandle: IDisposable;
  private readonly onCursorHandle: IDisposable;
  private readonly onSelectionHandle: IDisposable;
  private readonly beforeUnload: (event: BeforeUnloadEvent) => void;
  private readonly tryConnectId: number;
  private readonly resetFailuresId: number;

  // Client-server state
  private me: number = -1;
  private revision: number = 0;
  private outstanding?: OpSeq;
  private buffer?: OpSeq;
  private users: Map<number, { info: OnlineUser, cursor: CursorData }> = new Map();
  private myInfo?: OnlineUser;
  private cursorData: CursorData = { cursors: [], selections: [] };

  // Intermittent local editor state
  private lastValue: string = "";
  private ignoreChanges: boolean = false;
  private oldDecorations: string[] = [];

  constructor(readonly options: RustpadOptions) {
    // Initialize the Rust WASM module. This must be done before any `OpSeq` methods are called.
    initSync(wasmBuffer);

    this.model = options.editor.getModel()!;
    this.onChangeHandle = options.editor.onDidChangeModelContent((e) =>
      this.onChange(e),
    );
    const cursorUpdate = debounce(() => this.sendCursorData(), 20);
    this.onCursorHandle = options.editor.onDidChangeCursorPosition((e) => {
      this.onCursor(e);
      cursorUpdate();
    });
    this.onSelectionHandle = options.editor.onDidChangeCursorSelection((e) => {
      this.onSelection(e);
      cursorUpdate();
    });
    this.beforeUnload = (event: BeforeUnloadEvent) => {
      if (this.outstanding) {
        event.preventDefault();
        event.returnValue = "";
      } else {
        delete event.returnValue;
      }
    };
    window.addEventListener("beforeunload", this.beforeUnload);

    const interval = options.reconnectInterval ?? 1000;
    this.tryConnect();
    this.tryConnectId = window.setInterval(() => this.tryConnect(), interval);
    this.resetFailuresId = window.setInterval(
      () => (this.recentFailures = 0),
      15 * interval,
    );
  }

  /** Destroy this Rustpad instance and close any sockets. */
  dispose() {
    window.clearInterval(this.tryConnectId);
    window.clearInterval(this.resetFailuresId);
    this.onSelectionHandle.dispose();
    this.onCursorHandle.dispose();
    this.onChangeHandle.dispose();
    window.removeEventListener("beforeunload", this.beforeUnload);
    this.ws?.close();
  }

  /** Try to set the metadata of the editor, if connected. */
  setMeta(language?: string, visibility?: Visibility): boolean {
    this.send({ SetMeta: { language, visibility } });
    return this.ws !== undefined;
  }

  /** Set the user's information. */
  setInfo(info: OnlineUser) {
    this.myInfo = info;
    this.sendInfo();
  }

  /**
   * Attempts a WebSocket connection.
   *
   * Safety Invariant: Until this WebSocket connection is closed, no other
   * connections will be attempted because either `this.ws` or
   * `this.connecting` will be set to a truthy value.
   *
   * Liveness Invariant: After this WebSocket connection closes, either through
   * error or successful end, both `this.connecting` and `this.ws` will be set
   * to falsy values.
   */
  private tryConnect() {
    if (this.connecting || this.ws) return;
    this.connecting = true;
    console.info("connecting to", this.options.uri);
    const ws = new WebSocket(this.options.uri);
    ws.onopen = () => {
      console.info("connected to", this.options.uri);
      this.connecting = false;
      this.ws = ws;
      // Trigger connection handler later after receiving identity message
      this.users.clear();
      this.options.onChangeUsers?.(Object.fromEntries(this.users.entries().map(([id, u]) => [id, u.info])));
      this.sendInfo();
      this.sendCursorData();
      if (this.outstanding) {
        this.sendOperation(this.outstanding);
      }
    };
    ws.onclose = () => {
      if (this.ws) {
        console.warn("disconnected from", this.options.uri);
        this.ws = undefined;
        this.options.onDisconnected?.();
        if (++this.recentFailures >= 5) {
          // If we disconnect 5 times within 15 reconnection intervals, then the
          // client is likely desynchronized and needs to refresh.
          this.dispose();
          this.options.onDesynchronized?.();
        }
      } else {
        this.connecting = false;
      }
    };
    ws.onerror = (e) => {
      console.error("error connecting to", this.options.uri, e);
      this.dispose();
      this.options.onError?.(e);
    };
    ws.onmessage = ({ data }) => {
      if (typeof data === "string") {
        this.handleMessage(JSON.parse(data));
      }
    };
  }

  private handleMessage(msg: ServerMsg) {
    console.debug("received message", msg);
    if (msg.Identity !== undefined) {
      console.info("received identity", msg.Identity);
      this.me = msg.Identity.id;
      this.myInfo = msg.Identity.info;
      this.options.onConnected?.(this.myInfo);
    } else if (msg.History !== undefined) {
      const { start, operations } = msg.History;
      if (start > this.revision) {
        console.warn("History message has start greater than last operation.");
        this.ws?.close();
        return;
      }
      for (let i = this.revision - start; i < operations.length; i++) {
        let { id, operation } = operations[i]!;
        this.revision++;
        if (id === this.me) {
          this.serverAck();
        } else {
          operation = OpSeq.from_str(JSON.stringify(operation));
          this.applyServer(operation);
        }
      }
    } else if (msg.Meta !== undefined) {
      this.options.onChangeMeta?.(msg.Meta.language, msg.Meta.visibility);
    } else if (msg.UserInfo !== undefined) {
      const { id, user } = msg.UserInfo;
      if (id !== this.me) {
        this.users.set(id, { info: user, cursor: { cursors: [], selections: [] } });
        this.updateCursors();
        this.options.onChangeUsers?.(Object.fromEntries(this.users.entries().map(([id, u]) => [id, u.info])));
      } else {
        this.myInfo = user;
        this.options.onChangeMe?.(user);
      }
    } else if (msg.UserDisconnect !== undefined) {
      const { id } = msg.UserDisconnect;
      if (id !== this.me) {
        this.users.delete(id);
        this.updateCursors();
        this.options.onChangeUsers?.(Object.fromEntries(this.users.entries().map(([id, u]) => [id, u.info])));
      } else {
        // Disconnection, can happen if user document becomes private
        this.ws?.close();
      }
    } else if (msg.UserCursor !== undefined) {
      const { id, data } = msg.UserCursor;
      if (id !== this.me) {
        let user = this.users.get(id);
        if (user) {
          user.cursor = data;
          this.updateCursors();
        }
      }
    }
  }

  private serverAck() {
    if (!this.outstanding) {
      console.warn("Received serverAck with no outstanding operation.");
      return;
    }
    this.outstanding = this.buffer;
    this.buffer = undefined;
    if (this.outstanding) {
      this.sendOperation(this.outstanding);
    }
  }

  private applyServer(operation: OpSeq) {
    if (this.outstanding) {
      const pair = this.outstanding.transform(operation)!;
      this.outstanding = pair.first();
      operation = pair.second();
      if (this.buffer) {
        const pair = this.buffer.transform(operation)!;
        this.buffer = pair.first();
        operation = pair.second();
      }
    }
    this.applyOperation(operation);
  }

  private applyClient(operation: OpSeq) {
    if (!this.outstanding) {
      this.sendOperation(operation);
      this.outstanding = operation;
    } else if (!this.buffer) {
      this.buffer = operation;
    } else {
      this.buffer = this.buffer.compose(operation);
    }
    this.transformCursors(operation);
  }

  private sendOperation(operation: OpSeq) {
    const op = operation.to_string();
    this.send({ Edit: { revision: this.revision, operation: JSON.parse(op) } });
  }

  private sendInfo() {
    if (this.myInfo) {
      this.send({ ClientInfo: this.myInfo });
    }
  }

  private sendCursorData() {
    if (!this.buffer) {
      this.send({ CursorData: this.cursorData });
    }
  }

  private send(msg: ClientMsg) {
    console.debug("sending message", msg);
    this.ws?.send(JSON.stringify(msg));
  }

  private applyOperation(operation: OpSeq) {
    if (operation.is_noop()) return;

    this.ignoreChanges = true;
    const ops: (string | number)[] = JSON.parse(operation.to_string());
    let index = 0;

    for (const op of ops) {
      if (typeof op === "string") {
        // Insert
        const pos = unicodePosition(this.model, index);
        index += unicodeLength(op);
        this.model.pushEditOperations(
          this.options.editor.getSelections(),
          [
            {
              range: {
                startLineNumber: pos.lineNumber,
                startColumn: pos.column,
                endLineNumber: pos.lineNumber,
                endColumn: pos.column,
              },
              text: op,
              forceMoveMarkers: true,
            },
          ],
          () => null,
        );
      } else if (op >= 0) {
        // Retain
        index += op;
      } else {
        // Delete
        const chars = -op;
        var from = unicodePosition(this.model, index);
        var to = unicodePosition(this.model, index + chars);
        this.model.pushEditOperations(
          this.options.editor.getSelections(),
          [
            {
              range: {
                startLineNumber: from.lineNumber,
                startColumn: from.column,
                endLineNumber: to.lineNumber,
                endColumn: to.column,
              },
              text: "",
              forceMoveMarkers: true,
            },
          ],
          () => null,
        );
      }
    }

    this.lastValue = this.model.getValue();
    this.ignoreChanges = false;

    this.transformCursors(operation);
  }

  private transformCursors(operation: OpSeq) {
    for (const data of this.users.values().map((u) => u.cursor)) {
      data.cursors = data.cursors.map((c) => operation.transform_index(c));
      data.selections = data.selections.map(([s, e]) => [
        operation.transform_index(s),
        operation.transform_index(e),
      ]);
    }
    this.updateCursors();
  }

  private updateCursors() {
    const decorations: editor.IModelDeltaDecoration[] = [];

    for (const [id, data] of this.users.entries()) {
      const { hue, name } = data.info;
      generateCssStyles(hue);

      for (const cursor of data.cursor.cursors) {
        const position = unicodePosition(this.model, cursor);
        decorations.push({
          options: {
            className: `remote-cursor-${hue}`,
            stickiness: 1,
            zIndex: 2,
          },
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: position.lineNumber,
            endColumn: position.column,
          },
        });
      }
      for (const selection of data.cursor.selections) {
        const position = unicodePosition(this.model, selection[0]);
        const positionEnd = unicodePosition(this.model, selection[1]);
        decorations.push({
          options: {
            className: `remote-selection-${hue}`,
            hoverMessage: {
              value: name,
            },
            stickiness: 1,
            zIndex: 1,
          },
          range: {
            startLineNumber: position.lineNumber,
            startColumn: position.column,
            endLineNumber: positionEnd.lineNumber,
            endColumn: positionEnd.column,
          },
        });
      }
    }

    this.oldDecorations = this.model.deltaDecorations(
      this.oldDecorations,
      decorations,
    );
  }

  private onChange(event: editor.IModelContentChangedEvent) {
    if (!this.ignoreChanges) {
      const content = this.lastValue;
      const contentLength = unicodeLength(content);
      let offset = 0;

      let operation = OpSeq.new();
      operation.retain(contentLength);
      event.changes.sort((a, b) => b.rangeOffset - a.rangeOffset);
      for (const change of event.changes) {
        // The following dance is necessary to convert from UTF-16 indices (evil
        // encoding-dependent JavaScript representation) to portable Unicode
        // codepoint indices.
        const { text, rangeOffset, rangeLength } = change;
        const initialLength = unicodeLength(content.slice(0, rangeOffset));
        const deletedLength = unicodeLength(
          content.slice(rangeOffset, rangeOffset + rangeLength),
        );
        const restLength =
          contentLength + offset - initialLength - deletedLength;
        const changeOp = OpSeq.new();
        changeOp.retain(initialLength);
        changeOp.delete(deletedLength);
        changeOp.insert(text);
        changeOp.retain(restLength);
        operation = operation.compose(changeOp)!;
        offset += changeOp.target_len() - changeOp.base_len();
      }
      this.applyClient(operation);
      this.lastValue = this.model.getValue();
    }
  }

  private onCursor(event: editor.ICursorPositionChangedEvent) {
    const cursors = [event.position, ...event.secondaryPositions];
    this.cursorData.cursors = cursors.map((p) => unicodeOffset(this.model, p));
  }

  private onSelection(event: editor.ICursorSelectionChangedEvent) {
    const selections = [event.selection, ...event.secondarySelections];
    this.cursorData.selections = selections.map((s) => [
      unicodeOffset(this.model, s.getStartPosition()),
      unicodeOffset(this.model, s.getEndPosition()),
    ]);
  }
}

type UserOperation = {
  id: number;
  operation: any;
};

type CursorData = {
  cursors: number[];
  selections: [number, number][];
};

type ClientMsg = {
  Edit?: {
    revision: number;
    operation: any;
  };
  SetMeta?: {
    language?: string;
    visibility?: Visibility;
  };
  ClientInfo?: {
    name: string;
    hue: number;
  };
  CursorData?: CursorData;
};

type ServerMsg = {
  Identity?: {
    id: number;
    info?: OnlineUser;
  };
  History?: {
    start: number;
    operations: UserOperation[];
  };
  Meta?: {
    language: string;
    visibility: Visibility;
  };
  UserInfo?: {
    id: number;
    user: OnlineUser;
  };
  UserDisconnect?: {
    id: number;
  };
  UserCursor?: {
    id: number;
    data: CursorData;
  };
};

/** Returns the number of Unicode codepoints in a string. */
function unicodeLength(str: string): number {
  let length = 0;
  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  for (const c of str) ++length;
  return length;
}

/** Returns the number of Unicode codepoints before a position in the model. */
function unicodeOffset(model: editor.ITextModel, pos: IPosition): number {
  const value = model.getValue();
  const offsetUTF16 = model.getOffsetAt(pos);
  return unicodeLength(value.slice(0, offsetUTF16));
}

/** Returns the position after a certain number of Unicode codepoints. */
function unicodePosition(model: editor.ITextModel, offset: number): IPosition {
  const value = model.getValue();
  let offsetUTF16 = 0;
  for (const c of value) {
    // Iterate over Unicode codepoints
    if (offset <= 0) break;
    offsetUTF16 += c.length;
    offset -= 1;
  }
  return model.getPositionAt(offsetUTF16);
}

/** Cache for private use by `generateCssStyles()`. */
const generatedStyles = new Set<number>();

/** Add CSS styles for a remote user's cursor and selection. */
function generateCssStyles(hue: number) {
  if (!generatedStyles.has(hue)) {
    generatedStyles.add(hue);
    const css = `
      .monaco-editor .remote-selection-${hue} {
        background-color: hsla(${hue}, 90%, 80%, 0.5);
      }
      .monaco-editor .remote-cursor-${hue} {
        border-left: 2px solid hsl(${hue}, 90%, 25%);
      }
    `;
    const element = document.createElement("style");
    const text = document.createTextNode(css);
    element.appendChild(text);
    document.head.appendChild(element);
  }
}

export default Rustpad;
