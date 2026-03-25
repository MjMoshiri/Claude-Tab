import React, { createContext, useContext, useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

/** Default values for auto-focus settings */
const CONFIG_DEFAULTS: Record<string, unknown> = {
  "autoFocus.windowBringToFront": true,
  "autoFocus.aggressiveMode": false,
  "autoFocus.tabAutoSwitch": true,
  "autoFocus.inactivitySeconds": 5,
  "autoFocus.countdownSeconds": 3,
  "skillGroups": {},
  "autoAccept.enabled": false,
  "autoAccept.defaultPolicy": "",
  "autoAccept.model": "haiku",
  "autoAccept.mode": "permission",
};

interface ConfigContextValue {
  get: <T = unknown>(key: string, defaultValue?: T) => T;
  set: (key: string, value: unknown) => Promise<void>;
  save: () => Promise<void>;
  values: Record<string, unknown>;
  ready: boolean;
  dirty: boolean;
}

const ConfigContext = createContext<ConfigContextValue>({
  get: () => undefined as never,
  set: async () => {},
  save: async () => {},
  values: {},
  ready: false,
  dirty: false,
});

export function ConfigProvider({ children }: { children: React.ReactNode }) {
  const [values, setValues] = useState<Record<string, unknown>>({});
  const [ready, setReady] = useState(false);
  const [dirty, setDirty] = useState(false);

  // Load initial config values from backend
  useEffect(() => {
    const loadConfig = async () => {
      const loaded: Record<string, unknown> = {};
      for (const key of Object.keys(CONFIG_DEFAULTS)) {
        try {
          const value = await invoke<unknown | null>("get_config_value", { key });
          loaded[key] = value ?? CONFIG_DEFAULTS[key];
        } catch {
          loaded[key] = CONFIG_DEFAULTS[key];
        }
      }
      setValues(loaded);
      setReady(true);
    };
    loadConfig();
  }, []);

  const get = useCallback(
    <T = unknown>(key: string, defaultValue?: T): T => {
      if (key in values) {
        return values[key] as T;
      }
      if (defaultValue !== undefined) {
        return defaultValue;
      }
      return CONFIG_DEFAULTS[key] as T;
    },
    [values]
  );

  const set = useCallback(async (key: string, value: unknown): Promise<void> => {
    await invoke("set_config_value", { key, value });
    setValues((prev) => ({ ...prev, [key]: value }));
    setDirty(true);
    // Emit custom event for extensions to sync
    window.dispatchEvent(new CustomEvent("config-changed", { detail: { key, value } }));
    // Also sync to localStorage for immediate access by extensions
    localStorage.setItem(`config.${key}`, JSON.stringify(value));
  }, []);

  const save = useCallback(async (): Promise<void> => {
    await invoke("save_config");
    setDirty(false);
  }, []);

  return (
    <ConfigContext.Provider value={{ get, set, save, values, ready, dirty }}>
      {children}
    </ConfigContext.Provider>
  );
}

export function useConfig() {
  return useContext(ConfigContext);
}
