import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useSettings, useTheme } from "../hooks/useSettings";
import {
  api,
  chooseExportPath,
  confirmAction,
  onBackendEvent,
  openExternal,
  revealInFinder,
  toAppError,
} from "../lib/api";
import { ANTHROPIC_CONSOLE_URL, CLAUDE_CODE_URL, REPOSITORY_URL } from "../lib/constants";
import { formatBytes, isoDate, modelLabel } from "../lib/format";
import { DEFAULT_MODEL, MODEL_OPTIONS, supportsEffort, supportsTemperature } from "../lib/models";
import { DEFAULT_SHORTCUTS, findConflicts, resolveShortcuts, SHORTCUT_LABELS, SHORTCUT_ORDER } from "../lib/shortcuts";
import type {
  AppInfo,
  ClaudeCodeStatus,
  Connection,
  ConnectionTest,
  Effort,
  Settings,
  ShortcutAction,
  StorageInfo,
} from "../types";
import { ShortcutRecorder } from "./ShortcutRecorder";

type Section = "general" | "claude" | "memory" | "keyboard" | "about";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "general", label: "General" },
  { id: "claude", label: "Claude" },
  { id: "memory", label: "Memory" },
  { id: "keyboard", label: "Keyboard" },
  { id: "about", label: "About" },
];

function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="srow">
      <div className="srow-label">
        <span>{label}</span>
        {hint && <span className="srow-hint">{hint}</span>}
      </div>
      <div className="srow-control">{children}</div>
    </div>
  );
}

function Toggle({ checked, onChange, label }: { checked: boolean; onChange: (v: boolean) => void; label: string }) {
  return (
    <button role="switch" aria-checked={checked} aria-label={label} className="switch" onClick={() => onChange(!checked)}>
      <span className="switch-knob" />
    </button>
  );
}

function Segmented<T extends string>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} role="radio" aria-checked={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function SettingsApp() {
  const { settings, save } = useSettings();
  useTheme(settings);
  const [section, setSection] = useState<Section>("general");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [status, setStatus] = useState<{ tone: "ok" | "error"; text: string } | null>(null);

  const refreshInfo = useCallback(() => {
    api.getAppInfo().then(setInfo).catch(() => {});
  }, []);

  useEffect(() => {
    refreshInfo();
    api
      .takeSettingsSection()
      .then((s) => s && setSection(s as Section))
      .catch(() => {});
    const offs = [
      onBackendEvent<string>("tf://settings-section", (s) => setSection(s as Section)),
      onBackendEvent("tf://api-key-changed", refreshInfo),
      onBackendEvent("tf://shortcut-status", refreshInfo),
    ];
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey)) return;
      const i = Number(e.key) - 1;
      const target = SECTIONS[i];
      if (target) {
        e.preventDefault();
        setSection(target.id);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      offs.forEach((off) => off());
      window.removeEventListener("keydown", onKey);
    };
  }, [refreshInfo]);

  useEffect(() => {
    if (!status) return;
    const t = window.setTimeout(() => setStatus(null), 4000);
    return () => window.clearTimeout(t);
  }, [status]);

  const update = useCallback(
    async (mutate: (s: Settings) => Settings) => {
      if (!settings) return;
      try {
        await save(mutate(structuredClone(settings)));
        refreshInfo();
      } catch (e) {
        setStatus({ tone: "error", text: toAppError(e).message });
      }
    },
    [settings, save, refreshInfo],
  );

  if (!settings) return <div className="settings-loading">Loading…</div>;

  return (
    <div className="settings">
      <nav className="settings-nav" aria-label="Settings sections">
        <p className="settings-brand">Thoughtflow</p>
        {SECTIONS.map((s, i) => (
          <button
            key={s.id}
            className={`nav-item${section === s.id ? " nav-on" : ""}`}
            aria-current={section === s.id ? "page" : undefined}
            onClick={() => setSection(s.id)}
          >
            {s.label}
            <kbd>⌘{i + 1}</kbd>
          </button>
        ))}
      </nav>
      <main className="settings-main">
        {section === "general" && <GeneralSection settings={settings} info={info} update={update} />}
        {section === "claude" && <ClaudeSection settings={settings} info={info} update={update} onInfo={refreshInfo} />}
        {section === "memory" && <MemorySection settings={settings} update={update} setStatus={setStatus} />}
        {section === "keyboard" && <KeyboardSection settings={settings} info={info} update={update} />}
        {section === "about" && <AboutSection info={info} />}
        {status && (
          <p className={`settings-status settings-status-${status.tone}`} role="status">
            {status.text}
          </p>
        )}
      </main>
    </div>
  );
}

type Update = (mutate: (s: Settings) => Settings) => Promise<void>;

function GlobalShortcutRow({ settings, info, update }: { settings: Settings; info: AppInfo | null; update: Update }) {
  const shortcut = info?.shortcut;
  return (
    <Row
      label="Open Thoughtflow"
      hint={
        shortcut && !shortcut.registered ? (
          <span className="error-text">{shortcut.error}</span>
        ) : (
          "Works from any app. Press it again to close."
        )
      }
    >
      <ShortcutRecorder
        value={settings.general.globalShortcut}
        label="Global shortcut"
        requireModifier
        onChange={(acc) =>
          void update((s) => {
            s.general.globalShortcut = acc;
            return s;
          })
        }
      />
    </Row>
  );
}

function GeneralSection({ settings, info, update }: { settings: Settings; info: AppInfo | null; update: Update }) {
  const g = settings.general;
  return (
    <section>
      <h1>General</h1>
      <Row label="Launch at login" hint="Starts quietly in the menu bar.">
        <Toggle
          label="Launch at login"
          checked={g.launchAtLogin}
          onChange={(v) =>
            void update((s) => {
              s.general.launchAtLogin = v;
              return s;
            })
          }
        />
      </Row>
      <GlobalShortcutRow settings={settings} info={info} update={update} />
      <Row label="Widget position" hint="Where the widget appears when summoned.">
        <select
          value={g.widgetPosition}
          onChange={(e) =>
            void update((s) => {
              s.general.widgetPosition = e.target.value as Settings["general"]["widgetPosition"];
              return s;
            })
          }
        >
          <option value="remember">Where I last left it</option>
          <option value="center">Centered on the active screen</option>
          <option value="top">Top of the active screen</option>
        </select>
      </Row>
      <Row label="Theme">
        <Segmented
          label="Theme"
          value={g.theme}
          options={[
            { value: "system", label: "System" },
            { value: "light", label: "Light" },
            { value: "dark", label: "Dark" },
          ]}
          onChange={(v) =>
            void update((s) => {
              s.general.theme = v;
              return s;
            })
          }
        />
      </Row>
      <Row label="Hide when I click away" hint="Off keeps the widget floating until you close it.">
        <Toggle
          label="Hide when I click away"
          checked={g.hideOnBlur}
          onChange={(v) =>
            void update((s) => {
              s.general.hideOnBlur = v;
              return s;
            })
          }
        />
      </Row>
      <p className="fine">Reminders are off. Thoughtflow never interrupts you unless you open it.</p>
    </section>
  );
}

function ClaudeSection({
  settings,
  info,
  update,
  onInfo,
}: {
  settings: Settings;
  info: AppInfo | null;
  update: Update;
  onInfo: () => void;
}) {
  const c = settings.claude;
  const [key, setKey] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [test, setTest] = useState<{ state: "idle" | "testing" } | { state: "ok"; result: ConnectionTest } | { state: "error"; message: string }>({
    state: "idle",
  });
  const known = MODEL_OPTIONS.some((m) => m.id === c.model);
  const [custom, setCustom] = useState(!known);
  const apiKey = info?.apiKey;

  const saveKey = async () => {
    try {
      await api.setApiKey(key);
      setKey("");
      setKeyError(null);
      onInfo();
      void runTest();
    } catch (e) {
      setKeyError(toAppError(e).message);
    }
  };

  const removeKey = async () => {
    if (!(await confirmAction("Remove the API key from your Keychain?", "Remove API key", "Remove"))) return;
    try {
      await api.deleteApiKey();
      setTest({ state: "idle" });
      onInfo();
    } catch (e) {
      setKeyError(toAppError(e).message);
    }
  };

  const runTest = async () => {
    setTest({ state: "testing" });
    try {
      setTest({ state: "ok", result: await api.testConnection() });
    } catch (e) {
      setTest({ state: "error", message: toAppError(e).message });
    }
  };

  const setModel = (model: string) =>
    void update((s) => {
      s.claude.model = model.trim() || DEFAULT_MODEL;
      return s;
    });

  const temperatureOk = supportsTemperature(c.model);
  const effortOk = supportsEffort(c.model);
  const option = MODEL_OPTIONS.find((m) => m.id === c.model);

  const viaCli = c.connection === "claudeCode";

  return (
    <section>
      <h1>Claude</h1>
      <Row
        label="Connect with"
        hint={
          viaCli
            ? "Uses the Claude Code CLI on this Mac and its sign-in, such as your Claude subscription. No API key needed."
            : "Uses the Anthropic API with your own key."
        }
      >
        <Segmented<Connection>
          label="Connect with"
          value={c.connection}
          options={[
            { value: "api", label: "API key" },
            { value: "claudeCode", label: "Claude Code" },
          ]}
          onChange={(v) =>
            void update((s) => {
              s.claude.connection = v;
              return s;
            })
          }
        />
      </Row>

      {viaCli ? (
        <ClaudeCodePanel settings={settings} update={update} />
      ) : (
      <>
      <div className={`connection connection-${apiKey?.configured ? (test.state === "error" ? "bad" : "ok") : "none"}`}>
        <span className="connection-dot" aria-hidden="true" />
        <span>
          {!apiKey?.configured && "No API key yet."}
          {apiKey?.configured &&
            `Key ${apiKey.hint ?? ""} from ${apiKey.source === "keychain" ? "your Keychain" : "the ANTHROPIC_API_KEY environment variable"}.`}
          {test.state === "testing" && " Testing…"}
          {test.state === "ok" &&
            ` Connected in ${test.result.latencyMs} ms${test.result.modelAvailable ? ` · ${modelLabel(c.model)} is available.` : ` · ${c.model} isn't listed for this key.`}`}
          {test.state === "error" && ` ${test.message}`}
        </span>
        <button className="btn" onClick={() => void runTest()} disabled={!apiKey?.configured || test.state === "testing"}>
          Test connection
        </button>
      </div>

      <Row
        label="API key"
        hint={
          <>
            Stored in the macOS Keychain and used only by Thoughtflow's background process.{" "}
            <button className="link" onClick={() => void openExternal(ANTHROPIC_CONSOLE_URL)}>
              Get a key
            </button>
          </>
        }
      >
        <form
          className="key-form"
          onSubmit={(e) => {
            e.preventDefault();
            void saveKey();
          }}
        >
          <input
            type="password"
            value={key}
            autoComplete="off"
            spellCheck={false}
            placeholder={apiKey?.configured ? "Replace key…" : "sk-ant-…"}
            aria-label="Anthropic API key"
            onChange={(e) => setKey(e.target.value)}
          />
          <button className="btn" type="submit" disabled={!key.trim()}>
            Save
          </button>
          {apiKey?.source === "keychain" && (
            <button className="btn btn-quiet btn-danger" type="button" onClick={() => void removeKey()}>
              Remove
            </button>
          )}
        </form>
        {keyError && <p className="error-text">{keyError}</p>}
      </Row>
      </>
      )}

      <Row label="Model" hint={option?.note ?? "A custom model id."}>
        <div className="model-pick">
          <select
            value={custom ? "__custom" : c.model}
            onChange={(e) => {
              if (e.target.value === "__custom") setCustom(true);
              else {
                setCustom(false);
                setModel(e.target.value);
              }
            }}
          >
            {MODEL_OPTIONS.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
            <option value="__custom">Other model id…</option>
          </select>
          {custom && (
            <input
              defaultValue={c.model}
              list="tf-models"
              placeholder="claude-…"
              aria-label="Custom model id"
              onBlur={(e) => setModel(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && setModel(e.currentTarget.value)}
            />
          )}
          <datalist id="tf-models">
            {test.state === "ok" && test.result.models.map((m) => <option key={m.id} value={m.id} label={m.displayName} />)}
          </datalist>
        </div>
      </Row>

      <Row
        label="Response depth"
        hint={effortOk ? "Quick keeps replies fast; Thorough thinks longer before answering." : "This model doesn't support a depth setting."}
      >
        <Segmented<Effort>
          label="Response depth"
          value={c.effort}
          options={[
            { value: "low", label: "Quick" },
            { value: "medium", label: "Balanced" },
            { value: "high", label: "Thorough" },
          ]}
          onChange={(v) =>
            void update((s) => {
              s.claude.effort = v;
              return s;
            })
          }
        />
      </Row>

      {!viaCli && (
      <>
      <Row
        label="Temperature"
        hint={temperatureOk ? "Lower is more focused; higher is more varied." : "Newer Claude models manage this themselves; the setting is ignored."}
      >
        <div className="temp">
          <label className="check">
            <input
              type="checkbox"
              checked={c.temperature === null}
              disabled={!temperatureOk}
              onChange={(e) =>
                void update((s) => {
                  s.claude.temperature = e.target.checked ? null : 0.7;
                  return s;
                })
              }
            />
            Default
          </label>
          <input
            type="range"
            min={0}
            max={1}
            step={0.1}
            disabled={!temperatureOk || c.temperature === null}
            value={c.temperature ?? 0.7}
            aria-label="Temperature"
            onChange={(e) =>
              void update((s) => {
                s.claude.temperature = Number(e.target.value);
                return s;
              })
            }
          />
          <span className="temp-value">{c.temperature === null ? "—" : c.temperature.toFixed(1)}</span>
        </div>
      </Row>

      <Row label="Longest reply" hint="An upper bound; replies are usually far shorter.">
        <select
          value={c.maxTokens}
          onChange={(e) =>
            void update((s) => {
              s.claude.maxTokens = Number(e.target.value);
              return s;
            })
          }
        >
          {[4000, 8000, 16000, 32000].map((n) => (
            <option key={n} value={n}>
              {n.toLocaleString()} tokens
            </option>
          ))}
        </select>
      </Row>
      </>
      )}

      <details className="disclosure">
        <summary>What is sent to Anthropic</summary>
        <ul>
          <li>Only when you send (⌘↵, a mode shortcut, ⌘K, Regenerate, or Save plan/tasks).</li>
          <li>The current thought's conversation, the mode you chose, and today's date.</li>
          <li>Related notes only if you leave them selected under the composer.</li>
          <li>Never your other thoughts, files, clipboard, screen, or which apps you use.</li>
          {viaCli ? (
            <>
              <li>
                Through Claude Code, Thoughtflow runs <code>claude -p</code> in an empty folder with its tools, MCP servers,
                plugins, hooks, and CLAUDE.md turned off, and without saving a session to your Claude Code history.
              </li>
              <li>Checking the connection only reads Claude Code's version and sign-in; nothing is sent.</li>
            </>
          ) : (
            <li>Test connection only lists available models; no thought content is sent.</li>
          )}
        </ul>
      </details>
    </section>
  );
}

function ClaudeCodePanel({ settings, update }: { settings: Settings; update: Update }) {
  const [status, setStatus] = useState<ClaudeCodeStatus | null>(null);
  const [checking, setChecking] = useState(false);
  const [path, setPath] = useState(settings.claude.cliPath ?? "");

  const check = useCallback(async (cliPath: string | null) => {
    setChecking(true);
    try {
      setStatus(await api.claudeCodeStatus(cliPath));
    } catch (e) {
      setStatus({
        found: false,
        path: null,
        version: null,
        loggedIn: false,
        authMethod: null,
        account: null,
        problem: toAppError(e).message,
      });
    } finally {
      setChecking(false);
    }
  }, []);

  useEffect(() => {
    void check(settings.claude.cliPath);
  }, [check, settings.claude.cliPath]);

  const savePath = () =>
    void update((s) => {
      s.claude.cliPath = path.trim() || null;
      return s;
    });

  const ok = Boolean(status?.found && status.loggedIn);
  const version = `Claude Code ${(status?.version ?? "").replace(/\s*\(Claude Code\)\s*$/, "")}`.trim();
  const signIn = status?.authMethod === "claude.ai" ? "your Claude subscription" : status?.authMethod ?? "its sign-in";

  return (
    <>
      <div className={`connection connection-${status === null ? "none" : ok ? "ok" : "bad"}`}>
        <span className="connection-dot" aria-hidden="true" />
        <span>
          {checking && !status && "Looking for Claude Code…"}
          {status && ok && `${version} · signed in with ${signIn}${status.account ? ` (${status.account})` : ""}.`}
          {status && !ok && status.problem}
        </span>
        <button className="btn" onClick={() => void check(settings.claude.cliPath)} disabled={checking}>
          {checking ? "Checking…" : "Check again"}
        </button>
      </div>
      <Row
        label="Claude Code location"
        hint={
          <>
            Leave empty to find <code>claude</code> automatically
            {status?.path && !settings.claude.cliPath ? ` (found at ${status.path})` : ""}. Not installed?{" "}
            <button className="link" onClick={() => void openExternal(CLAUDE_CODE_URL)}>
              Get Claude Code
            </button>
          </>
        }
      >
        <form
          className="key-form"
          onSubmit={(e) => {
            e.preventDefault();
            savePath();
          }}
        >
          <input
            value={path}
            spellCheck={false}
            placeholder="Automatic"
            aria-label="Path to the claude executable"
            onChange={(e) => setPath(e.target.value)}
          />
          <button className="btn" type="submit" disabled={path.trim() === (settings.claude.cliPath ?? "")}>
            Save
          </button>
        </form>
      </Row>
    </>
  );
}

function MemorySection({
  settings,
  update,
  setStatus,
}: {
  settings: Settings;
  update: Update;
  setStatus: (s: { tone: "ok" | "error"; text: string } | null) => void;
}) {
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const load = useCallback(() => {
    api.getStorageInfo().then(setStorage).catch(() => {});
  }, []);
  useEffect(load, [load]);

  const exportData = async () => {
    const path = await chooseExportPath(`thoughtflow-export-${isoDate()}.json`);
    if (!path) return;
    try {
      const saved = await api.exportData(path);
      setStatus({ tone: "ok", text: `Exported to ${saved}` });
    } catch (e) {
      setStatus({ tone: "error", text: toAppError(e).message });
    }
  };

  const clearHistory = async () => {
    const ok = await confirmAction(
      "Permanently delete every thought, conversation, plan, task, and prompt? Settings and your API key are kept. This can't be undone.",
      "Clear history",
      "Delete everything",
    );
    if (!ok) return;
    try {
      await api.clearHistory();
      load();
      setStatus({ tone: "ok", text: "History cleared." });
    } catch (e) {
      setStatus({ tone: "error", text: toAppError(e).message });
    }
  };

  const deleteAll = async () => {
    const ok = await confirmAction(
      "Delete all Thoughtflow data on this Mac: thoughts, plans, tasks, settings, and the API key in your Keychain? This can't be undone.",
      "Delete all data",
      "Delete all data",
    );
    if (!ok) return;
    try {
      await api.deleteAllData();
      load();
      setStatus({ tone: "ok", text: "All Thoughtflow data was deleted." });
    } catch (e) {
      setStatus({ tone: "error", text: toAppError(e).message });
    }
  };

  return (
    <section>
      <h1>Memory</h1>
      <Row
        label="Offer related notes"
        hint="When you start a thought, Thoughtflow searches your past notes on this Mac and suggests up to three. They're sent to Claude only if you leave them selected."
      >
        <Toggle
          label="Offer related notes"
          checked={settings.claude.useMemory}
          onChange={(v) =>
            void update((s) => {
              s.claude.useMemory = v;
              return s;
            })
          }
        />
      </Row>
      <Row
        label="Storage"
        hint={storage ? `${storage.thoughts} thoughts · ${storage.plans} plans · ${storage.tasks} tasks · ${formatBytes(storage.dbBytes)}` : ""}
      >
        <div className="path-row">
          <code className="path" title={storage?.dbPath}>
            {storage?.dataDir ?? "…"}
          </code>
          <button className="btn" onClick={() => storage && void revealInFinder(storage.dbPath)}>
            Reveal in Finder
          </button>
        </div>
      </Row>
      <Row label="Export" hint="Everything you've saved, as readable JSON.">
        <button className="btn" onClick={() => void exportData()}>
          Export data…
        </button>
      </Row>
      <Row label="Clear history" hint="Deletes thoughts, conversations, plans, tasks, and prompts.">
        <button className="btn btn-danger" onClick={() => void clearHistory()}>
          Clear history…
        </button>
      </Row>
      <Row label="Delete all data" hint="Also resets settings and removes the API key from your Keychain.">
        <button className="btn btn-danger" onClick={() => void deleteAll()}>
          Delete all data…
        </button>
      </Row>
      <p className="fine">
        To keep one thought out of Claude's context, open it and choose ⋯ → Keep out of Claude's memory. Deleting a
        thought removes it from disk immediately.
      </p>
    </section>
  );
}

function KeyboardSection({ settings, info, update }: { settings: Settings; info: AppInfo | null; update: Update }) {
  const bindings = resolveShortcuts(settings.keyboard.shortcuts);
  const conflicts = new Set(findConflicts(bindings));
  const setBinding = (action: ShortcutAction, acc: string) =>
    void update((s) => {
      s.keyboard.shortcuts = { ...s.keyboard.shortcuts, [action]: acc };
      return s;
    });

  return (
    <section>
      <h1>Keyboard</h1>
      <h2 className="label">Global</h2>
      <GlobalShortcutRow settings={settings} info={info} update={update} />
      <h2 className="label">In the widget</h2>
      {SHORTCUT_ORDER.map((action) => (
        <Row
          key={action}
          label={SHORTCUT_LABELS[action]}
          hint={conflicts.has(action) ? <span className="error-text">Same as another shortcut</span> : undefined}
        >
          <ShortcutRecorder
            value={bindings[action]}
            label={SHORTCUT_LABELS[action]}
            requireModifier={false}
            onChange={(acc) => setBinding(action, acc)}
          />
        </Row>
      ))}
      <div className="row-end">
        <button
          className="btn"
          onClick={() =>
            void update((s) => {
              s.keyboard.shortcuts = {};
              return s;
            })
          }
          disabled={SHORTCUT_ORDER.every((a) => bindings[a] === DEFAULT_SHORTCUTS[a])}
        >
          Restore defaults
        </button>
      </div>
    </section>
  );
}

function AboutSection({ info }: { info: AppInfo | null }) {
  return (
    <section className="about">
      <h1>Thoughtflow</h1>
      <p className="about-version">Version {info?.version ?? "…"}</p>
      <p>
        A quiet thinking layer over your desktop. Capture a thought, think it through with Claude, and leave with a
        plan, a few tasks, or a prompt you can use anywhere.
      </p>
      <p>
        <button className="link" onClick={() => void openExternal(REPOSITORY_URL)}>
          {REPOSITORY_URL.replace("https://", "")}
        </button>
      </p>
      <p className="fine">
        Your thoughts are stored only on this Mac{info ? ` (${info.dataDir})` : ""}. Text is sent to Anthropic only when
        you ask Claude for something. MIT License.
      </p>
    </section>
  );
}
