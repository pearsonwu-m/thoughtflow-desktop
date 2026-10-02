// Typed wrappers over the Tauri command surface (src-tauri/src/commands.rs).
//
// The webview never sees the API key or touches the database directly; every
// operation goes through these commands. When the UI runs in a plain browser
// (`npm run dev:web`), calls are served by an in-memory mock instead so the
// interface can be developed without the Rust backend.

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  ApiKeyStatus,
  AppError,
  AppInfo,
  ConnectionTest,
  Mode,
  NewTask,
  Plan,
  Prompt,
  RelatedNote,
  SendRequest,
  Settings,
  ShortcutStatus,
  StorageInfo,
  StreamEvent,
  Task,
  Thought,
  ThoughtDetail,
} from "../types";

export const inTauri = isTauri();

export function toAppError(e: unknown): AppError {
  if (e && typeof e === "object" && "message" in e && "kind" in e) {
    const err = e as Partial<AppError>;
    return { kind: String(err.kind), message: String(err.message), retryable: Boolean(err.retryable) };
  }
  if (typeof e === "string") return { kind: "internal", message: e, retryable: false };
  if (e instanceof Error) return { kind: "internal", message: e.message, retryable: false };
  return { kind: "internal", message: "Something went wrong.", retryable: false };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    if (!inTauri) {
      const mock = await import("./mock");
      return await mock.mockInvoke<T>(command, args ?? {});
    }
    return await invoke<T>(command, args);
  } catch (e) {
    throw toAppError(e);
  }
}

function streamChannel(onEvent: (e: StreamEvent) => void): Channel<StreamEvent> | ((e: StreamEvent) => void) {
  if (!inTauri) return onEvent;
  const channel = new Channel<StreamEvent>();
  channel.onmessage = onEvent;
  return channel;
}

export const api = {
  // App & settings
  getAppInfo: () => call<AppInfo>("get_app_info"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (settings: Settings) => call<Settings>("update_settings", { settings }),
  getShortcutStatus: () => call<ShortcutStatus>("get_shortcut_status"),

  // API key (write-only from the UI's point of view)
  getApiKeyStatus: () => call<ApiKeyStatus>("get_api_key_status"),
  setApiKey: (key: string) => call<ApiKeyStatus>("set_api_key", { key }),
  deleteApiKey: () => call<ApiKeyStatus>("delete_api_key"),
  testConnection: () => call<ConnectionTest>("test_connection"),

  // Windows
  showWidget: (view?: string) => call<null>("show_widget", { view: view ?? null }),
  hideWidget: () => call<null>("hide_widget"),
  resizeWidget: (height: number) => call<null>("resize_widget", { height }),
  openSettings: (section?: string) => call<null>("open_settings", { section: section ?? null }),
  takeSettingsSection: () => call<string | null>("take_settings_section"),
  quit: () => call<null>("quit_app"),

  // Thoughts
  listThoughts: (query?: string, limit?: number) =>
    call<Thought[]>("list_thoughts", { query: query ?? null, limit: limit ?? null }),
  getThought: (id: string) => call<ThoughtDetail>("get_thought", { id }),
  captureThought: (text: string, mode: Mode) => call<ThoughtDetail>("capture_thought", { text, mode }),
  renameThought: (id: string, title: string) => call<Thought>("rename_thought", { id, title }),
  setThoughtTags: (id: string, tags: string[]) => call<Thought>("set_thought_tags", { id, tags }),
  setThoughtMemory: (id: string, include: boolean) => call<Thought>("set_thought_memory", { id, include }),
  deleteThought: (id: string) => call<boolean>("delete_thought", { id }),
  findRelated: (text: string, excludeId: string | null) =>
    call<RelatedNote[]>("find_related", { text, excludeId }),

  // Claude
  sendMessage: (request: SendRequest, onEvent: (e: StreamEvent) => void) =>
    call<ThoughtDetail>("send_message", { request, onEvent: streamChannel(onEvent) }),
  regenerate: (thoughtId: string, requestId: string, onEvent: (e: StreamEvent) => void) =>
    call<ThoughtDetail>("regenerate_response", { thoughtId, requestId, onEvent: streamChannel(onEvent) }),
  cancelMessage: (requestId: string) => call<boolean>("cancel_message", { requestId }),
  extractPlan: (thoughtId: string, today: string) => call<Plan>("extract_plan", { thoughtId, today }),
  extractTasks: (thoughtId: string, today: string) => call<Task[]>("extract_tasks", { thoughtId, today }),
  updatePrompt: (id: string, content: string) => call<Prompt>("update_prompt", { id, content }),

  // Plans & tasks
  listPlans: () => call<Plan[]>("list_plans"),
  updatePlan: (plan: Plan) => call<Plan>("update_plan", { plan }),
  deletePlan: (id: string) => call<boolean>("delete_plan", { id }),
  listTasks: () => call<Task[]>("list_tasks"),
  createTask: (task: NewTask) => call<Task>("create_task", { task }),
  updateTask: (task: Task) => call<Task>("update_task", { task }),
  deleteTask: (id: string) => call<boolean>("delete_task", { id }),

  // Data
  getStorageInfo: () => call<StorageInfo>("get_storage_info"),
  exportData: (path: string) => call<string>("export_data", { path }),
  clearHistory: () => call<null>("clear_history"),
  deleteAllData: () => call<null>("delete_all_data"),
};

/** Subscribes to a backend event; returns an unsubscribe function. */
export function onBackendEvent<T>(name: string, handler: (payload: T) => void): () => void {
  if (!inTauri) {
    let off = () => {};
    void import("./mock").then((mock) => {
      off = mock.mockListen(name, handler as (p: unknown) => void);
    });
    return () => off();
  }
  let disposed = false;
  let unlisten: (() => void) | null = null;
  void listen<T>(name, (event) => handler(event.payload)).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });
  return () => {
    disposed = true;
    unlisten?.();
  };
}

/** Which window this webview is: the floating widget or settings. */
export async function currentWindowLabel(): Promise<"widget" | "settings"> {
  if (!inTauri) {
    return new URLSearchParams(window.location.search).get("window") === "settings" ? "settings" : "widget";
  }
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow().label === "settings" ? "settings" : "widget";
}

export async function copyText(text: string): Promise<void> {
  if (inTauri) {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
  } else {
    await navigator.clipboard.writeText(text);
  }
}

export async function openExternal(url: string): Promise<void> {
  if (inTauri) {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } else {
    window.open(url, "_blank", "noopener");
  }
}

export async function revealInFinder(path: string): Promise<void> {
  if (!inTauri) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export async function chooseExportPath(defaultName: string): Promise<string | null> {
  if (!inTauri) return `/tmp/${defaultName}`;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return save({ defaultPath: defaultName, filters: [{ name: "JSON", extensions: ["json"] }] });
}

export async function confirmAction(message: string, title: string, okLabel: string): Promise<boolean> {
  if (!inTauri) return window.confirm(message);
  const { ask } = await import("@tauri-apps/plugin-dialog");
  return ask(message, { title, kind: "warning", okLabel, cancelLabel: "Cancel" });
}

export function newRequestId(): string {
  return crypto.randomUUID();
}
