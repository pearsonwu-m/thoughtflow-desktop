// Shared types. These mirror the Rust structs in src-tauri/src (serialized
// with camelCase field names), so keep the two in sync.

export type Mode = "think" | "plan" | "task" | "reflect" | "prompt";

export const MODES: readonly Mode[] = ["think", "plan", "task", "reflect", "prompt"];

export interface Thought {
  id: string;
  title: string;
  rawInput: string;
  summary: string;
  tags: string[];
  mode: Mode;
  includeInMemory: boolean;
  createdAt: number;
  updatedAt: number;
}

export type MessageRole = "user" | "assistant" | "system";
export type MessageKind = "message" | "clarify" | "modeRequest" | "modeSwitch";

/** One turn of a conversation, as the UI sees it. */
export interface Message {
  id: string;
  thoughtId: string;
  seq: number;
  role: MessageRole;
  kind: MessageKind;
  mode: Mode;
  text: string;
  model: string | null;
  contextIds: string[];
  truncated: boolean;
  createdAt: number;
}

/** A thought together with its conversation. */
export interface Conversation {
  thought: Thought;
  messages: Message[];
}

export interface ThoughtDetail extends Conversation {
  plans: Plan[];
  tasks: Task[];
  prompts: Prompt[];
}

export interface PlanStep {
  title: string;
  detail: string;
  done: boolean;
}

export type PlanStatus = "active" | "done" | "archived";

export interface Plan {
  id: string;
  thoughtId: string | null;
  title: string;
  objective: string;
  why: string;
  steps: PlanStep[];
  nextAction: string;
  obstacles: string[];
  deadline: string | null;
  status: PlanStatus;
  createdAt: number;
  updatedAt: number;
}

export type Priority = "high" | "medium" | "low";
export type TaskEffort = "quick" | "short" | "medium" | "long";

export interface Task {
  id: string;
  thoughtId: string | null;
  planId: string | null;
  name: string;
  description: string;
  priority: Priority;
  effort: TaskEffort;
  dueDate: string | null;
  completed: boolean;
  completedAt: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface NewTask {
  name: string;
  description?: string;
  priority?: Priority;
  effort?: TaskEffort;
  dueDate?: string | null;
  thoughtId?: string | null;
}

/** A generated prompt from Prompt Mode (possibly edited by the user). */
export interface Prompt {
  id: string;
  thoughtId: string;
  messageId: string | null;
  content: string;
  edited: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface RelatedNote {
  id: string;
  title: string;
  createdAt: number;
  snippet: string;
}

export type Effort = "low" | "medium" | "high";
export type WidgetPosition = "remember" | "center" | "top";
export type Theme = "system" | "light" | "dark";

export interface Settings {
  general: {
    launchAtLogin: boolean;
    globalShortcut: string;
    widgetPosition: WidgetPosition;
    theme: Theme;
    hideOnBlur: boolean;
  };
  claude: {
    model: string;
    effort: Effort;
    temperature: number | null;
    maxTokens: number;
    useMemory: boolean;
  };
  keyboard: {
    /** User overrides of in-app shortcuts, keyed by action. */
    shortcuts: Partial<Record<ShortcutAction, string>>;
  };
  widget: { x: number | null; y: number | null };
}

export type ShortcutAction =
  | "submit"
  | "close"
  | "clarify"
  | "planMode"
  | "taskMode"
  | "reflectMode"
  | "promptMode"
  | "thinkMode"
  | "history"
  | "settings"
  | "newThought"
  | "save"
  | "stop";

export type KeySource = "keychain" | "environment" | "none";

export interface ApiKeyStatus {
  configured: boolean;
  source: KeySource;
  hint: string | null;
}

export interface ShortcutStatus {
  accelerator: string;
  registered: boolean;
  error: string | null;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  dbPath: string;
  shortcut: ShortcutStatus;
  apiKey: ApiKeyStatus;
  startupWarning: string | null;
}

export interface ModelInfo {
  id: string;
  displayName: string;
}

export interface ConnectionTest {
  latencyMs: number;
  models: ModelInfo[];
  modelAvailable: boolean;
}

export interface StorageInfo {
  dataDir: string;
  dbPath: string;
  dbBytes: number;
  thoughts: number;
  plans: number;
  tasks: number;
}

export type SendKind = "message" | "clarify" | "modeRequest";

export interface SendRequest {
  requestId: string;
  thoughtId: string | null;
  text: string;
  mode: Mode;
  kind: SendKind;
  contextIds: string[];
  localDate: string;
}

/** Progress events streamed while Claude replies. */
export type StreamEvent =
  | { type: "sending"; model: string }
  | { type: "started"; model: string }
  | { type: "thinking" }
  | { type: "retrying"; attempt: number; delayMs: number; reason: string }
  | { type: "delta"; text: string };

/** The error shape every backend command rejects with. */
export interface AppError {
  kind: string;
  message: string;
  retryable: boolean;
}
