import { useCallback, useEffect, useState } from "react";
import { api, onBackendEvent } from "../lib/api";
import { resolveShortcuts } from "../lib/shortcuts";
import type { AppError, Settings, ShortcutAction } from "../types";

/** Loads settings and stays in sync when any window changes them. */
export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loadError, setLoadError] = useState<AppError | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .getSettings()
      .then((s) => alive && setSettings(s))
      .catch((e: AppError) => alive && setLoadError(e));
    const off = onBackendEvent<Settings>("tf://settings-changed", (s) => setSettings(s));
    return () => {
      alive = false;
      off();
    };
  }, []);

  const save = useCallback(async (next: Settings) => {
    const saved = await api.updateSettings(next);
    setSettings(saved);
    return saved;
  }, []);

  return { settings, setSettings, save, loadError };
}

export function useBindings(settings: Settings | null): Record<ShortcutAction, string> {
  return resolveShortcuts(settings?.keyboard.shortcuts);
}

/** Applies the theme preference to <html data-theme>. */
export function useTheme(settings: Settings | null) {
  const preference = settings?.general.theme ?? "system";
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = preference === "dark" || (preference === "system" && media.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [preference]);
}
