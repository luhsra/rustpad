import debounce from "lodash.debounce";
import Quill, { Delta, type Range } from "quill";

/** Options passed in to the Rustpad constructor. */
export type RustpadOptions = {
  readonly uri: string;
  readonly editor: Quill;
  readonly onConnected?: (info?: OnlineUser) => void;
  readonly onDisconnected?: () => void;
  readonly onDesynchronized?: () => void;
  readonly onError?: (error: Event) => void;
  readonly onChangeMeta?: (visibility: Visibility) => void;
  readonly onChangeUsers?: (users: Record<number, OnlineUser>) => void;
  readonly onChangeMe?: (info: OnlineUser) => void;
  readonly reconnectInterval?: number;
};

export type UserRole = "admin" | "user" | "anon";
export type Visibility = "public" | "internal" | "private";

export function canAccess(role: UserRole, visibility: Visibility): boolean {
  if (visibility === "public") return true;
  if (visibility === "internal") return role !== "anon";
  return role === "admin";
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
  private connecting = false;
  private recentFailures = 0;
  private readonly editor: Quill;
  private readonly beforeUnload: (event: BeforeUnloadEvent) => void;
  private readonly tryConnectId: number;
  private readonly resetFailuresId: number;
  private readonly cursorUpdate: ReturnType<typeof debounce>;
  private readonly overlays: HTMLDivElement;

  private me = -1;
  private revision = 0;
  private outstanding?: Delta;
  private buffer?: Delta;
  private users = new Map<number, { info: OnlineUser; cursor: CursorData }>();
  private myInfo?: OnlineUser;
  private cursorData: CursorData = { cursors: [], selections: [] };

  constructor(readonly options: RustpadOptions) {
    this.editor = options.editor;
    this.overlays = document.createElement("div");
    this.overlays.className = "remote-overlays";
    this.editor.container.appendChild(this.overlays);

    this.cursorUpdate = debounce(() => this.sendCursorData(), 20);
    this.editor.on("text-change", this.onChange);
    this.editor.on("selection-change", this.onSelection);
    this.editor.root.addEventListener("scroll", this.updateCursors);
    window.addEventListener("resize", this.updateCursors);

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
    this.cursorUpdate.cancel();
    this.editor.off("text-change", this.onChange);
    this.editor.off("selection-change", this.onSelection);
    this.editor.root.removeEventListener("scroll", this.updateCursors);
    window.removeEventListener("resize", this.updateCursors);
    window.removeEventListener("beforeunload", this.beforeUnload);
    this.overlays.remove();
    this.ws?.close();
  }

  /** Try to set the visibility of the editor, if connected. */
  setVisibility(visibility: Visibility): boolean {
    this.send({ SetMeta: { visibility } });
    return this.ws !== undefined;
  }

  /** Set the user's information. */
  setInfo(info: OnlineUser) {
    this.myInfo = info;
    this.sendInfo();
  }

  private tryConnect() {
    if (this.connecting || this.ws) return;
    this.connecting = true;
    const ws = new WebSocket(this.options.uri);
    ws.onopen = () => {
      this.connecting = false;
      this.ws = ws;
      this.users.clear();
      this.notifyUsersChanged();
      this.sendInfo();
      this.sendCursorData();
      if (this.outstanding) this.sendOperation(this.outstanding);
    };
    ws.onclose = () => {
      if (this.ws) {
        this.ws = undefined;
        this.options.onDisconnected?.();
        if (++this.recentFailures >= 5) {
          this.dispose();
          this.options.onDesynchronized?.();
        }
      } else {
        this.connecting = false;
      }
    };
    ws.onerror = (event) => {
      this.dispose();
      this.options.onError?.(event);
    };
    ws.onmessage = ({ data }) => {
      if (typeof data === "string") this.handleMessage(JSON.parse(data));
    };
  }

  private handleMessage(msg: ServerMsg) {
    if (msg.Identity !== undefined) {
      this.me = msg.Identity.id;
      this.myInfo = msg.Identity.info;
      this.options.onConnected?.(this.myInfo);
    } else if (msg.History !== undefined) {
      const { start, operations } = msg.History;
      if (start > this.revision) {
        this.ws?.close();
        return;
      }
      for (let i = this.revision - start; i < operations.length; i++) {
        let { id, operation } = operations[i]!;
        this.revision++;
        if (id === this.me) {
          this.serverAck();
        } else {
          this.applyServer(new Delta(operation));
        }
      }
    } else if (msg.Meta !== undefined) {
      this.options.onChangeMeta?.(msg.Meta.visibility);
    } else if (msg.UserInfo !== undefined) {
      const { id, user } = msg.UserInfo;
      if (id !== this.me) {
        this.users.set(id, {
          info: user,
          cursor: { cursors: [], selections: [] },
        });
        this.updateCursors();
        this.notifyUsersChanged();
      } else {
        this.myInfo = user;
        this.options.onChangeMe?.(user);
      }
    } else if (msg.UserDisconnect !== undefined) {
      const { id } = msg.UserDisconnect;
      if (id !== this.me) {
        this.users.delete(id);
        this.updateCursors();
        this.notifyUsersChanged();
      } else {
        this.ws?.close();
      }
    } else if (msg.UserCursor !== undefined && msg.UserCursor.id !== this.me) {
      const user = this.users.get(msg.UserCursor.id);
      if (user) {
        user.cursor = msg.UserCursor.data;
        this.updateCursors();
      }
    }
  }

  private notifyUsersChanged() {
    this.options.onChangeUsers?.(
      Object.fromEntries(
        this.users.entries().map(([id, user]) => [id, user.info]),
      ),
    );
  }

  private serverAck() {
    if (!this.outstanding) return;
    this.outstanding = this.buffer;
    this.buffer = undefined;
    if (this.outstanding) this.sendOperation(this.outstanding);
  }

  private applyServer(operation: Delta) {
    if (this.outstanding) {
      const outstanding = this.outstanding;
      this.outstanding = operation.transform(outstanding, true);
      operation = outstanding.transform(operation, false);
      if (this.buffer) {
        const buffer = this.buffer;
        this.buffer = operation.transform(buffer, true);
        operation = buffer.transform(operation, false);
      }
    }
    if (operation.ops.length === 0) return;
    this.editor.updateContents(operation, "api");
    this.transformCursors(operation);
  }

  private applyClient(operation: Delta) {
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

  private sendOperation(operation: Delta) {
    this.send({ Edit: { revision: this.revision, operation } });
  }

  private sendInfo() {
    if (this.myInfo) this.send({ ClientInfo: this.myInfo });
  }

  private sendCursorData() {
    if (!this.buffer) this.send({ CursorData: this.cursorData });
  }

  private send(msg: ClientMsg) {
    this.ws?.send(JSON.stringify(msg));
  }

  private transformCursors(operation: Delta) {
    for (const data of this.users.values().map((user) => user.cursor)) {
      data.cursors = data.cursors.map((cursor) =>
        operation.transformPosition(cursor),
      );
      data.selections = data.selections.map(([start, end]) => [
        operation.transformPosition(start),
        operation.transformPosition(end),
      ]);
    }
    this.updateCursors();
  }

  private updateCursors = () => {
    this.overlays.replaceChildren();
    for (const data of this.users.values()) {
      const { hue, name } = data.info;
      for (const cursor of data.cursor.cursors) {
        const bounds = this.editor.getBounds(cursor, 0);
        if (!bounds) continue;
        const marker = document.createElement("div");
        marker.className = "remote-cursor";
        marker.style.left = `${bounds.left}px`;
        marker.style.top = `${bounds.top}px`;
        marker.style.height = `${bounds.height}px`;
        marker.style.backgroundColor = `hsl(${hue}, 90%, 35%)`;
        marker.style.borderColor = `hsl(${hue}, 90%, 35%)`;
        marker.dataset.name = name;
        this.overlays.appendChild(marker);
      }
      for (const [start, end] of data.cursor.selections) {
        if (end <= start) continue;
        const bounds = this.editor.getBounds(start, end - start);
        if (!bounds) continue;
        const selection = document.createElement("div");
        selection.className = "remote-selection";
        selection.style.left = `${bounds.left}px`;
        selection.style.top = `${bounds.top}px`;
        selection.style.width = `${Math.max(bounds.width, 2)}px`;
        selection.style.height = `${bounds.height}px`;
        selection.style.backgroundColor = `hsla(${hue}, 90%, 60%, 0.25)`;
        selection.title = name;
        this.overlays.appendChild(selection);
      }
    }
  };

  private onChange = (operation: Delta, _old: Delta, source: string) => {
    if (source === "user") this.applyClient(operation);
  };

  private onSelection = (
    range: Range | null,
    _old: Range | null,
    source: string,
  ) => {
    if (source === "silent") return;
    if (range) {
      this.cursorData.cursors = [range.index + range.length];
      this.cursorData.selections = [[range.index, range.index + range.length]];
    } else {
      this.cursorData = { cursors: [], selections: [] };
    }
    this.cursorUpdate();
  };
}

type UserOperation = {
  id: number;
  operation: { ops: Record<string, unknown>[] };
};

type CursorData = {
  cursors: number[];
  selections: [number, number][];
};

type ClientMsg = {
  Edit?: { revision: number; operation: Delta };
  SetMeta?: { visibility?: Visibility };
  ClientInfo?: { name: string; hue: number };
  CursorData?: CursorData;
};

type ServerMsg = {
  Identity?: { id: number; info?: OnlineUser };
  History?: { start: number; operations: UserOperation[] };
  Meta?: { visibility: Visibility };
  UserInfo?: { id: number; user: OnlineUser };
  UserDisconnect?: { id: number };
  UserCursor?: { id: number; data: CursorData };
};

export default Rustpad;
