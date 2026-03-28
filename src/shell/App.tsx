import { useEffect, useRef, useState, useMemo } from "react";
import { SlotRenderer } from "./SlotRenderer";
import { ErrorBoundary } from "./ErrorBoundary";
import { EventBus } from "../kernel/EventBus";
import { ExtensionHost } from "../kernel/ExtensionHost";
import { ComponentRegistry } from "../kernel/ComponentRegistry";
import { KeybindingManager } from "../kernel/KeybindingManager";
import { ConfigProvider } from "../kernel/ConfigProvider";
import { EventBusProvider } from "../kernel/EventBusContext";
import { FocusManager } from "../kernel/FocusManager";
import { SessionStateManager } from "../kernel/SessionStateManager";
import { FocusProvider } from "../kernel/FocusContext";
import { SessionStateProvider } from "../kernel/SessionStateContext";
import { KeybindingManagerProvider } from "../kernel/KeybindingManagerContext";
import { SLOTS } from "../types/slots";
import { TauriBackendService } from "../services/TauriBackendService";
import { TauriEventListener } from "../services/TauriEventListener";

import { createTabBarExtension } from "../extensions/tab-bar";
import { createTerminalPanelExtension } from "../extensions/terminal-panel";
import { createStatusBarExtension } from "../extensions/status-bar";
import { createCountdownTimerExtension } from "../extensions/countdown-timer";
import { createCommandPaletteExtension } from "../extensions/command-palette";
import { createSettingsExtension } from "../extensions/settings";
import { createProfilesExtension } from "../extensions/profiles";
import { createProfileLauncherExtension } from "../extensions/profile-launcher";
import { createWindowFocusExtension } from "../extensions/window-focus";
import { createInactivitySwitchExtension } from "../extensions/inactivity-switch";
import { createPolicyBadgeExtension } from "../extensions/policy-badge";
import { createUpdateNotifierExtension } from "../extensions/update-notifier";

/**
 * Kernel instances - created once and persisted for the app lifetime.
 * Using useMemo with empty deps to ensure single instantiation.
 */
function useKernel() {
  return useMemo(() => {
    const registry = new ComponentRegistry();
    const eventBus = new EventBus();
    const keybindingManager = new KeybindingManager();
    const backend = new TauriBackendService();
    const eventListener = new TauriEventListener();
    const focusManager = new FocusManager();
    const sessionStateManager = new SessionStateManager();

    const extensionHost = new ExtensionHost({
      eventBus,
      componentRegistry: registry,
      keybindingManager,
      backend,
      eventListener,
      focusManager,
      sessionStateManager,
    });

    // Register all extensions
    extensionHost.register(createTabBarExtension());
    extensionHost.register(createTerminalPanelExtension());
    extensionHost.register(createStatusBarExtension());
    extensionHost.register(createCountdownTimerExtension());
    extensionHost.register(createCommandPaletteExtension());
    extensionHost.register(createSettingsExtension());
    extensionHost.register(createProfilesExtension());
    extensionHost.register(createProfileLauncherExtension());
    extensionHost.register(createWindowFocusExtension());
    extensionHost.register(createInactivitySwitchExtension());
    extensionHost.register(createPolicyBadgeExtension());
    extensionHost.register(createUpdateNotifierExtension());

    return {
      registry,
      eventBus,
      keybindingManager,
      backend,
      eventListener,
      focusManager,
      sessionStateManager,
      extensionHost,
    };
  }, []);
}

function useRegistryUpdates(registry: ComponentRegistry) {
  const [, setTick] = useState(0);
  useEffect(() => {
    return registry.subscribe(() => setTick((t) => t + 1));
  }, [registry]);
}

export function App() {
  const [ready, setReady] = useState(false);
  const [initError, setInitError] = useState<string | null>(null);
  const kernel = useKernel();
  useRegistryUpdates(kernel.registry);
  const cleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const {
      eventBus,
      keybindingManager,
      eventListener,
      focusManager,
      sessionStateManager,
      extensionHost,
    } = kernel;

    // Wire up FocusManager so SessionStateManager can query inactivity in tick()
    sessionStateManager.setFocusManager(focusManager);

    // Store cleanup function
    cleanupRef.current = () => {
      extensionHost.deactivateAll();
      keybindingManager.destroy();
      eventBus.destroy();
      eventListener.destroy();
      focusManager.destroy();
      sessionStateManager.destroy();
    };

    // Initialize async
    (async () => {
      try {
        await eventBus.init();
        await focusManager.init();
        await sessionStateManager.init();
        await extensionHost.activateAll();
        setReady(true);
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        console.error("[App] Init failed:", err);
        setInitError(message);
      }
    })();

    return () => {
      if (cleanupRef.current) {
        cleanupRef.current();
        cleanupRef.current = null;
      }
    };
  }, [kernel]);

  if (initError) {
    return (
      <div style={{ padding: 20, color: "#ff6b6b", fontFamily: "monospace" }}>
        <h2>Initialization Failed</h2>
        <pre>{initError}</pre>
        <button
          onClick={() => window.location.reload()}
          style={{ marginTop: 10, padding: "8px 16px", cursor: "pointer" }}
        >
          Reload
        </button>
      </div>
    );
  }

  if (!ready) {
    return <div className="app-loading">Loading...</div>;
  }

  const { registry, eventBus, keybindingManager, focusManager, sessionStateManager } = kernel;

  return (
    <ErrorBoundary>
      <EventBusProvider eventBus={eventBus}>
        <KeybindingManagerProvider manager={keybindingManager}>
          <FocusProvider manager={focusManager}>
            <SessionStateProvider manager={sessionStateManager}>
              <ConfigProvider>
              <div className="app-container">
                <header className="app-header">
                  <div className="slot-left">
                    <SlotRenderer registry={registry} slot={SLOTS.TAB_BAR_LEFT} />
                  </div>
                  <div className="slot-center">
                    <SlotRenderer registry={registry} slot={SLOTS.TAB_BAR_CENTER} />
                  </div>
                  <div className="slot-right">
                    <SlotRenderer registry={registry} slot={SLOTS.TAB_BAR_RIGHT} />
                  </div>
                </header>

                <main className="app-main">
                  <aside className="app-sidebar">
                    <SlotRenderer registry={registry} slot={SLOTS.SIDE_PANEL} />
                  </aside>
                  <div className="app-content">
                    <SlotRenderer registry={registry} slot={SLOTS.MAIN_CONTENT} />
                    <SlotRenderer registry={registry} slot={SLOTS.TERMINAL_OVERLAY} />
                  </div>
                </main>

                <footer className="app-footer">
                  <div className="slot-left">
                    <SlotRenderer registry={registry} slot={SLOTS.STATUS_BAR_LEFT} />
                  </div>
                  <div className="slot-center">
                    <SlotRenderer registry={registry} slot={SLOTS.STATUS_BAR_CENTER} />
                  </div>
                  <div className="slot-right">
                    <SlotRenderer registry={registry} slot={SLOTS.STATUS_BAR_RIGHT} />
                  </div>
                </footer>

                <SlotRenderer registry={registry} slot={SLOTS.OVERLAY} />
              </div>
              </ConfigProvider>
            </SessionStateProvider>
          </FocusProvider>
        </KeybindingManagerProvider>
      </EventBusProvider>
    </ErrorBoundary>
  );
}
