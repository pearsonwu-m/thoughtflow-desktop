// In-memory stand-in for the Rust backend, used only when the UI runs in a
// plain browser (`npm run dev:web`). It lets you iterate on the interface
// without Tauri or an API key. Replies are canned, not from Claude.

import type {
  AppInfo,
  Message,
  Mode,
  Plan,
  Prompt,
  SendRequest,
  Settings,
  StreamEvent,
  Task,
  Thought,
  ThoughtDetail,
} from "../types";

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();

export function mockListen(name: string, handler: Listener): () => void {
  const set = listeners.get(name) ?? new Set();
  set.add(handler);
  listeners.set(name, set);
  return () => set.delete(handler);
}

function emit(name: string, payload: unknown) {
  listeners.get(name)?.forEach((fn) => fn(payload));
}

const now = Date.now();
const id = () => crypto.randomUUID();

let settings: Settings = {
  general: { launchAtLogin: false, globalShortcut: "Alt+Space", widgetPosition: "remember", theme: "system", hideOnBlur: false },
  claude: {
    connection: "api",
    cliPath: null,
    model: "claude-opus-5-5",
    effort: "low",
    temperature: null,
    maxTokens: 16000,
    useMemory: true,
  },
  keyboard: { shortcuts: {} },
  widget: { x: null, y: null },
};

const thoughts: Thought[] = [
  ["Physics study plan", "Problem set 4 is due Friday and I haven't started the optics section.", 2.5, "plan"],
  ["Website ideas", "Portfolio site: maybe a notes section, keep it minimal, ship before applications.", 5, "think"],
  ["Email Dr. Lignos about research", "Need to ask about the summer lab position and send my transcript.", 27, "task"],
  ["Reading list for the break", "Foucault, Arendt, maybe Byung-Chul Han.", 80, "think"],
].map(([title, raw, hoursAgo, mode]) => ({
  id: id(),
  title: title as string,
  rawInput: raw as string,
  summary: raw as string,
  tags: [],
  mode: mode as Mode,
  includeInMemory: true,
  createdAt: now - (hoursAgo as number) * 3_600_000,
  updatedAt: now - (hoursAgo as number) * 3_600_000,
}));
const messages = new Map<string, Message[]>();
const plans: Plan[] = [];
const tasks: Task[] = [];
const prompts: Prompt[] = [];
const cancelled = new Set<string>();

function detail(thoughtId: string): ThoughtDetail {
  const thought = thoughts.find((t) => t.id === thoughtId);
  if (!thought) throw { kind: "notFound", message: "That thought no longer exists.", retryable: false };
  return {
    thought: { ...thought },
    messages: [...(messages.get(thoughtId) ?? [])],
    plans: plans.filter((p) => p.thoughtId === thoughtId),
    tasks: tasks.filter((t) => t.thoughtId === thoughtId),
    prompts: prompts.filter((p) => p.thoughtId === thoughtId),
  };
}

function reply(mode: Mode, kind: string, turn: number): string {
  if (kind === "clarify") return "**Question:** What would make this feel finished by the end of the week — and which part has a deadline someone else set?\n\nThat tells us what to protect first.";
  switch (mode) {
    case "plan":
      return "### Objective\nFinish the physics problem set and send the email to Dr. Lignos by Friday.\n\n### Why it matters\nThe problem set is graded and the research reply is time-sensitive.\n\n### Steps\n1. Block 90 minutes tonight for optics problems 1–4\n2. Draft the email with two specific questions\n3. Finish problems 5–8 on Wednesday\n4. Review answers and submit Thursday\n\n### Next action\nOpen the problem set and read problem 1 (10 minutes).\n\n### Possible obstacles\n- Optics concepts are shaky → skim lecture 12 notes first\n- Email feels awkward → send a short version, not a perfect one\n\n### Deadline\nFriday";
    case "task":
      return "- **Start optics problems 1–4**: read lecture 12 notes first · Priority: high · Effort: short · Due: Wednesday\n- **Email Dr. Lignos**: ask about the summer lab and attach transcript · Priority: high · Effort: quick\n- **Sketch website notes section** · Priority: low · Effort: medium\n\nStart with the email: it takes ten minutes and unblocks a reply.";
    case "reflect":
      return "I notice two patterns.\n\n- **Deadlines set by others get postponed until they're urgent**, while the website (no deadline) keeps returning. It seems like the open-ended work is the one you actually want to do.\n- **\"I'm already busy\" appears in most notes**, but the plans rarely say what you'd drop.\n\n**Question:** If you could only protect one of these this week, which would it be?";
    case "prompt":
      return turn < 1
        ? "**Question:** What should the other AI produce?\n\n- an academic research question\n- an essay outline\n- a full research proposal\n\nReply with one, or say “just generate it”."
        : "<prompt>\nYou are a political theorist who studies digital media in China.\n\nObjective: Help me develop a research question connecting Foucault's concept of panopticism to “keyboard politics” on the Chinese internet.\n\nContext: I'm writing an undergraduate essay. I know Discipline and Punish but have no specialist background on Chinese platforms.\n\nPlease:\n1. Explain panopticism in two paragraphs, citing the specific chapter.\n2. Summarize how scholars describe online expression on Chinese platforms; cite only real, verifiable sources and flag uncertainty.\n3. Propose three candidate research questions, each with the tension it explores.\n\nFormat: headings for each part, under 800 words. Ask me questions first if anything is ambiguous.\n</prompt>\nAssumed: undergraduate essay; English sources.";
    default:
      return "Got it — three different kinds of work are competing for the same evenings.\n\n**What I'm hearing:** You have a graded deadline (physics), a time-sensitive message (Dr. Lignos), and a project you care about but nobody is waiting on (the website).\n\n**The tension:** The website is the one you want to do, so it pulls attention from the two that have consequences.\n\n**Question:** Which of these has the earliest hard deadline?\n\n**Possible directions:**\n- **Clear the email first** — ten minutes, and it starts a clock you can't control\n- **Timebox physics** — two focused blocks before Friday\n- **Protect one website hour** — as a reward, not a guilt item";
  }
}

async function sendMock(req: SendRequest, onEvent: (e: StreamEvent) => void, regenerate = false): Promise<ThoughtDetail> {
  if (!req.text.trim() && req.kind === "message" && !regenerate) throw { kind: "invalid", message: "Write a thought first.", retryable: false };
  onEvent({
    type: "sending",
    model: settings.claude.model,
    via: settings.claude.connection === "claudeCode" ? "claude-code" : "api",
  });
  await delay(350);
  onEvent({ type: "started", model: settings.claude.model });
  onEvent({ type: "thinking" });
  await delay(500);
  let thought = req.thoughtId ? thoughts.find((t) => t.id === req.thoughtId) : undefined;
  const history = thought ? messages.get(thought.id) ?? [] : [];
  const turn = history.filter((m) => m.role === "assistant" && m.mode === req.mode).length;
  const text = reply(req.mode, req.kind, turn);
  for (const chunk of text.match(/[\s\S]{1,14}/g) ?? []) {
    if (cancelled.delete(req.requestId)) throw { kind: "cancelled", message: "Stopped.", retryable: false };
    onEvent({ type: "delta", text: chunk });
    await delay(18);
  }
  const at = Date.now();
  if (!thought) {
    thought = {
      id: id(),
      title: req.text.split(/[.\n]/)[0]?.slice(0, 56) || "Untitled thought",
      rawInput: req.text,
      summary: "",
      tags: [],
      mode: req.mode,
      includeInMemory: true,
      createdAt: at,
      updatedAt: at,
    };
    thoughts.unshift(thought);
  }
  const list = messages.get(thought.id) ?? [];
  const base = { thoughtId: thought.id, model: null, contextIds: [], truncated: false, createdAt: at };
  if (regenerate) list.pop();
  else {
    list.push({ ...base, id: id(), seq: list.length, role: "user", kind: req.kind, mode: req.mode, text: req.text || "(request)" });
    if (thought.mode !== req.mode || list.length === 1)
      list.push({ ...base, id: id(), seq: list.length, role: "system", kind: "modeSwitch", mode: req.mode, text: "" });
  }
  const assistant: Message = { ...base, id: id(), seq: list.length, role: "assistant", kind: "message", mode: req.mode, text, model: settings.claude.model };
  list.push(assistant);
  messages.set(thought.id, list);
  const block = /<prompt>([\s\S]*?)<\/prompt>/.exec(text);
  if (block?.[1]) prompts.push({ id: id(), thoughtId: thought.id, messageId: assistant.id, content: block[1].trim(), edited: false, createdAt: at, updatedAt: at });
  thought.mode = req.mode;
  thought.updatedAt = at;
  return detail(thought.id);
}

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function mockInvoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  // The mock trusts its callers (src/lib/api.ts), so arguments are loosely typed.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const a: Record<string, any> = args;
  const result = await (async (): Promise<unknown> => {
    switch (command) {
      case "get_app_info":
        return {
          version: "0.1.0 (browser preview)",
          dataDir: "~/Library/Application Support/com.thoughtflow.desktop",
          dbPath: "~/Library/Application Support/com.thoughtflow.desktop/thoughtflow.db",
          shortcut: { accelerator: settings.general.globalShortcut, registered: true, error: null },
          apiKey: { configured: true, source: "keychain", hint: "sk-ant-…demo" },
          startupWarning: null,
        } satisfies AppInfo;
      case "get_settings":
        return settings;
      case "update_settings":
        settings = a.settings;
        emit("tf://settings-changed", settings);
        return settings;
      case "get_api_key_status":
        return { configured: true, source: "keychain", hint: "sk-ant-…demo" };
      case "set_api_key":
      case "delete_api_key":
        return { configured: command === "set_api_key", source: command === "set_api_key" ? "keychain" : "none", hint: null };
      case "claude_code_status":
        await delay(250);
        return {
          found: true,
          path: "~/.local/bin/claude",
          version: "2.1.287 (Claude Code)",
          loggedIn: true,
          authMethod: "claude.ai",
          account: "you@example.com",
          problem: null,
        };
      case "test_connection":
        await delay(300);
        return { latencyMs: 286, modelAvailable: true, models: [{ id: "claude-opus-5-5", displayName: "Claude Opus 5.5" }] };
      case "list_thoughts": {
        const q = String(a.query ?? "").toLowerCase();
        return thoughts
          .filter((t) => !q || `${t.title} ${t.rawInput}`.toLowerCase().includes(q))
          .sort((x, y) => y.updatedAt - x.updatedAt);
      }
      case "get_thought":
        return detail(a.id);
      case "capture_thought": {
        const t: Thought = { id: id(), title: String(a.text).slice(0, 56), rawInput: a.text, summary: "", tags: [], mode: a.mode, includeInMemory: true, createdAt: Date.now(), updatedAt: Date.now() };
        thoughts.unshift(t);
        return detail(t.id);
      }
      case "rename_thought":
      case "set_thought_tags":
      case "set_thought_memory": {
        const t = thoughts.find((x) => x.id === a.id);
        if (!t) throw { kind: "notFound", message: "That thought no longer exists.", retryable: false };
        if (command === "rename_thought") t.title = a.title;
        if (command === "set_thought_tags") t.tags = a.tags;
        if (command === "set_thought_memory") t.includeInMemory = a.include;
        return t;
      }
      case "delete_thought": {
        const i = thoughts.findIndex((t) => t.id === a.id);
        if (i >= 0) thoughts.splice(i, 1);
        return i >= 0;
      }
      case "find_related": {
        const words = String(a.text).toLowerCase().split(/\W+/).filter((w) => w.length > 4);
        return thoughts
          .filter((t) => t.id !== a.excludeId && words.some((w) => `${t.title} ${t.rawInput}`.toLowerCase().includes(w)))
          .slice(0, 3)
          .map((t) => ({ id: t.id, title: t.title, createdAt: t.createdAt, snippet: t.rawInput }));
      }
      case "send_message":
        return sendMock(a.request, a.onEvent as unknown as (e: StreamEvent) => void);
      case "regenerate_response": {
        const list = messages.get(a.thoughtId) ?? [];
        const last = list.at(-1);
        return sendMock(
          { requestId: a.requestId, thoughtId: a.thoughtId, text: "", mode: last?.mode ?? "think", kind: "message", contextIds: [], localDate: "" },
          a.onEvent as unknown as (e: StreamEvent) => void,
          true,
        );
      }
      case "cancel_message":
        cancelled.add(a.requestId);
        return true;
      case "extract_plan": {
        await delay(600);
        const plan: Plan = {
          id: id(), thoughtId: a.thoughtId, title: "Physics and email by Friday", objective: "Finish the problem set and email Dr. Lignos by Friday.",
          why: "Both have outside deadlines.", steps: [
            { title: "Block 90 minutes for optics problems 1–4", detail: "", done: false },
            { title: "Draft the email to Dr. Lignos", detail: "", done: false },
            { title: "Finish problems 5–8", detail: "", done: false },
          ], nextAction: "Open the problem set and read problem 1.", obstacles: ["Shaky optics concepts"], deadline: null,
          status: "active", createdAt: Date.now(), updatedAt: Date.now(),
        };
        plans.push(plan);
        return plan;
      }
      case "extract_tasks": {
        await delay(600);
        const made: Task[] = ["Start optics problems 1–4", "Email Dr. Lignos"].map((name, i) => ({
          id: id(), thoughtId: a.thoughtId, planId: null, name, description: "", priority: i ? "high" : "medium", effort: i ? "quick" : "short",
          dueDate: null, completed: false, completedAt: null, createdAt: Date.now(), updatedAt: Date.now(),
        }));
        tasks.push(...made);
        return made;
      }
      case "update_prompt": {
        const p = prompts.find((x) => x.id === a.id);
        if (p) Object.assign(p, { content: a.content, edited: true });
        return p;
      }
      case "list_plans":
        return plans;
      case "update_plan": {
        const i = plans.findIndex((p) => p.id === (a.plan as Plan).id);
        if (i >= 0) plans[i] = a.plan;
        return a.plan;
      }
      case "delete_plan":
      case "delete_task": {
        const list: { id: string }[] = command === "delete_plan" ? plans : tasks;
        const i = list.findIndex((x) => x.id === a.id);
        if (i >= 0) list.splice(i, 1);
        return i >= 0;
      }
      case "list_tasks":
        return [...tasks].sort((x, y) => Number(x.completed) - Number(y.completed));
      case "create_task": {
        const t = a.task as { name: string };
        const task: Task = { id: id(), thoughtId: null, planId: null, name: t.name, description: "", priority: "medium", effort: "short", dueDate: null, completed: false, completedAt: null, createdAt: Date.now(), updatedAt: Date.now() };
        tasks.push(task);
        return task;
      }
      case "update_task": {
        const i = tasks.findIndex((t) => t.id === (a.task as Task).id);
        if (i >= 0) tasks[i] = a.task;
        return a.task;
      }
      case "get_storage_info":
        return { dataDir: "~/Library/Application Support/com.thoughtflow.desktop", dbPath: "thoughtflow.db", dbBytes: 98_304, thoughts: thoughts.length, plans: plans.length, tasks: tasks.length };
      case "export_data":
        return a.path;
      case "clear_history":
      case "delete_all_data":
        thoughts.splice(0);
        emit("tf://data-cleared", null);
        return null;
      case "take_settings_section":
        return null;
      case "resize_widget":
      case "show_widget":
      case "open_settings":
      case "quit_app":
        return null;
      case "hide_widget":
        emit("tf://hiding", null);
        await delay(150);
        emit("tf://shown", { view: null });
        return null;
      default:
        throw { kind: "internal", message: `Mock has no command ${command}`, retryable: false };
    }
  })();
  return result as T;
}
