/**
 * Kernel Component Interfaces
 *
 * These interfaces define the contracts for all kernel components,
 * enabling dependency injection and component replacement without
 * changing functionality.
 */

import { CoreEvent, EventHandler, UnsubscribeFn } from "./events";
import { SlotComponent } from "./extension";
import { SlotName } from "./slots";

// ============================================================================
// Event Bus Interface
// ============================================================================

type EventMiddleware = (event: CoreEvent) => boolean;

/**
 * Event distribution system with wildcard pattern matching.
 * Patterns: "*" (all), "session.*" (one level), "session.**" (deep match)
 */
export interface IEventBus {
  /** Initialize the event bus (e.g., set up Tauri listeners) */
  init(): Promise<void>;

  /** Subscribe to events matching a pattern */
  subscribe(pattern: string, handler: EventHandler): UnsubscribeFn;

  /** Publish an event to local handlers */
  publish(event: CoreEvent): void;

  /** Add middleware to filter events before dispatch */
  addMiddleware(mw: EventMiddleware): void;

  /** Emit an event to the backend */
  emitToBackend(topic: string, payload: Record<string, unknown>): Promise<void>;

  /** Clean up resources */
  destroy(): void;
}

// ============================================================================
// Component Registry Interface
// ============================================================================

type RegistryListener = () => void;

/**
 * Slot-based UI component registration system.
 * Components are registered into named slots and sorted by priority.
 */
export interface IComponentRegistry {
  /** Register a component in a slot */
  register(slot: SlotName, component: SlotComponent): void;

  /** Unregister a component from a slot */
  unregister(slot: SlotName, componentId: string): void;

  /** Get all components registered in a slot, sorted by priority */
  getComponents(slot: SlotName): SlotComponent[];

  /** Subscribe to registry changes */
  subscribe(listener: RegistryListener): UnsubscribeFn;
}

// ============================================================================
// Keybinding Manager Interface
// ============================================================================

export interface KeybindingDefinition {
  readonly id: string;
  keys: string;
  defaultKeys?: string;
  readonly label: string;
  readonly handler: () => void;
  readonly extensionId: string;
  readonly when?: string;
}

/**
 * Global keyboard shortcut management with customization support.
 */
export interface IKeybindingManager {
  /** Register a keybinding, returns unregister function */
  register(binding: KeybindingDefinition): UnsubscribeFn;

  /** Unregister a keybinding by ID */
  unregister(id: string): void;

  /** Update the keys for a keybinding */
  updateKeys(id: string, newKeys: string): void;

  /** Reset a keybinding to its default keys */
  resetKeys(id: string): void;

  /** Enable or disable keybinding processing */
  setActive(active: boolean): void;

  /** Get all registered keybindings */
  getAll(): KeybindingDefinition[];

  /** Check if a key string is already bound */
  hasBinding(keyStr: string): boolean;

  /** Convert a keyboard event to a key string (e.g., "Cmd+K") */
  eventToKeyString(e: KeyboardEvent): string;

  /** Clean up resources */
  destroy(): void;
}

// ============================================================================
// Config Provider Interface
// ============================================================================

/**
 * Configuration access with layered precedence.
 */
export interface IConfigProvider {
  /** Get a configuration value */
  get(key: string): unknown;

  /** Set a configuration value */
  set(key: string, value: unknown): Promise<void>;

  /** Get all current configuration values */
  values: Record<string, unknown>;
}

// ============================================================================
// Focus Manager Interface
// ============================================================================

export interface FocusState {
  /** Whether the window is currently focused */
  windowFocused: boolean;
  /** Timestamp of last user activity */
  lastActivityTime: number;
}

/**
 * Centralized window focus state management.
 * Provides native platform-specific focus operations.
 */
export interface IFocusManager {
  /** Current focus state */
  readonly state: FocusState;

  /** Whether the window is currently focused */
  readonly isWindowFocused: boolean;

  /** Timestamp of last user activity */
  readonly lastActivityTime: number;

  /** Seconds since last user activity */
  readonly inactivitySeconds: number;

  /** Initialize the focus manager */
  init(): Promise<void>;

  /** Record user activity, resetting the inactivity timer */
  recordActivity(): void;

  /** Focus the application window using native platform APIs */
  focusWindow(): Promise<void>;

  /** Request user attention (dock bounce on macOS, taskbar flash on Windows) */
  requestAttention(critical?: boolean): Promise<void>;

  /** Check if the app is currently active/frontmost */
  isAppActive(): Promise<boolean>;

  /** Subscribe to focus state changes */
  subscribe(listener: (state: FocusState) => void): () => void;

  /** Clean up resources */
  destroy(): void;
}

// ============================================================================
// Session State Manager Interface
// ============================================================================

export type SwitchState =
  | "idle"
  | "checking_inactivity"
  | "showing_toast"
  | "switching";

export interface ToastState {
  readonly targetSessionId: string;
  readonly targetSessionName: string;
  readonly countdownSeconds: number;
}

/**
 * State machine for session switching to prevent race conditions.
 *
 * Tracks "seen" sessions — once a user acknowledges a your_turn session
 * (by dismissing the toast, visiting the tab, or completing the switch),
 * that session won't be re-offered until its state cycles through a
 * non-your_turn state and back.
 */
export interface ISessionStateManager {
  /** Current state machine state */
  readonly state: SwitchState;

  /** Current toast state (if showing) */
  readonly toastState: ToastState | null;

  /** Initialize the manager */
  init(): Promise<void>;

  /** Set the FocusManager reference for tick-based inactivity checks */
  setFocusManager(focusManager: IFocusManager): void;

  /** Update configuration values */
  updateConfig(config: { enabled?: boolean; inactivitySeconds?: number; countdownSeconds?: number }): void;

  /** Check inactivity and potentially trigger toast */
  checkInactivity(inactivitySeconds: number): void;

  /** Complete the switch (toast countdown finished) */
  completeSwitch(): Promise<void>;

  /** Dismiss the toast and mark session as seen */
  dismissToast(): void;

  /** Subscribe to state changes */
  subscribe(listener: (state: SwitchState, toast: ToastState | null) => void): () => void;

  /** Clean up resources */
  destroy(): void;
}
