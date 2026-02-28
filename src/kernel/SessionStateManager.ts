/**
 * SessionStateManager - State machine for session switching
 *
 * Manages the inactivity-based tab switching flow with a state machine
 * to prevent race conditions. States:
 *
 *   idle → checking_inactivity → showing_toast → switching → idle
 *                                     ↓
 *                                    idle (dismissed)
 *
 * Tracks "seen" sessions: once a user acknowledges a your_turn/completed
 * session (by dismissing the toast, visiting the tab, or completing the
 * switch), that session won't be re-offered until its state changes
 * (indicating it genuinely needs attention again).
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ISessionStateManager, SwitchState, ToastState } from "../types/kernel";
import type { IFocusManager } from "../types/kernel";
import type { SessionInfo } from "../types/session";

/** Session states that indicate "needs user attention" for auto-switch purposes */
const ATTENTION_STATES = new Set(["your_turn", "completed"]);

type StateChangeListener = (state: SwitchState, toast: ToastState | null) => void;

export class SessionStateManager implements ISessionStateManager {
  private _state: SwitchState = "idle";
  private _toastState: ToastState | null = null;
  private _seenSessionIds = new Set<string>();
  private listeners = new Set<StateChangeListener>();
  private checkInterval: ReturnType<typeof setInterval> | null = null;
  private refreshLock = false;
  private refreshDebounceTimeout: ReturnType<typeof setTimeout> | null = null;
  private unsubCoreEvent: (() => void) | null = null;
  private destroyed = false;

  private cachedSessions: SessionInfo[] = [];
  private cachedActiveSessionId: string | null = null;

  private config = {
    enabled: true,
    inactivitySeconds: 5,
    countdownSeconds: 3,
  };

  private focusManager: IFocusManager | null = null;

  setFocusManager(focusManager: IFocusManager): void {
    this.focusManager = focusManager;
  }

  get state(): SwitchState {
    return this._state;
  }

  get toastState(): ToastState | null {
    return this._toastState ? { ...this._toastState } : null;
  }

  async init(): Promise<void> {
    this.unsubCoreEvent = await listen<{ topic: string; payload: Record<string, unknown> }>(
      "core-event",
      (e) => {
        const { topic, payload } = e.payload;
        const sessionId = payload.session_id as string;

        if (topic === "session.active_changed" && sessionId) {
          // User switched to this session — mark as seen unconditionally.
          // The state_changed handler clears seen status on state transitions,
          // so this won't prevent re-offering after genuine state changes.
          this._seenSessionIds.add(sessionId);
        }

        if (topic === "session.state_changed" && sessionId) {
          // State changed — remove from seen so it can be re-offered
          this._seenSessionIds.delete(sessionId);

          // If the toast is showing for this session and its state just changed,
          // dismiss the toast — the information is stale
          if (
            this._state === "showing_toast" &&
            this._toastState?.targetSessionId === sessionId
          ) {
            this._toastState = null;
            this.transitionTo("idle");
          }
        }

        if (topic === "session.closed" && sessionId) {
          this._seenSessionIds.delete(sessionId);

          // Dismiss toast if it was for the closed session
          if (
            this._state === "showing_toast" &&
            this._toastState?.targetSessionId === sessionId
          ) {
            this._toastState = null;
            this.transitionTo("idle");
          }
        }

        if (
          topic === "session.created" ||
          topic === "session.closed" ||
          topic === "session.state_changed" ||
          topic === "session.active_changed"
        ) {
          this.refreshSessionsDebounced();
        }
      }
    );

    await this.refreshSessions();

    this.checkInterval = setInterval(() => this.tick(), 1000);
  }

  updateConfig(config: Partial<typeof this.config>): void {
    this.config = { ...this.config, ...config };
  }

  checkInactivity(inactivitySeconds: number): void {
    if (this._state !== "idle") return;
    if (!this.config.enabled) return;
    if (inactivitySeconds < this.config.inactivitySeconds) return;

    this.evaluateSwitch().catch((err) => {
      console.error("[SessionStateManager] evaluateSwitch failed:", err);
    });
  }

  async completeSwitch(): Promise<void> {
    if (this._state !== "showing_toast" || !this._toastState) return;

    const targetSessionId = this._toastState.targetSessionId;
    this._seenSessionIds.add(targetSessionId);
    this._toastState = null;
    this.transitionTo("switching");

    try {
      await invoke("set_active_session", { sessionId: targetSessionId });
    } catch (err) {
      console.warn("[SessionStateManager] Failed to switch session:", err);
    }

    this.transitionTo("idle");
  }

  dismissToast(): void {
    if (this._state !== "showing_toast" || !this._toastState) return;

    this._seenSessionIds.add(this._toastState.targetSessionId);
    this._toastState = null;
    this.transitionTo("idle");
  }

  subscribe(listener: StateChangeListener): () => void {
    this.listeners.add(listener);
    listener(this._state, this.toastState);
    return () => {
      this.listeners.delete(listener);
    };
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;

    if (this.checkInterval) {
      clearInterval(this.checkInterval);
      this.checkInterval = null;
    }

    if (this.unsubCoreEvent) {
      this.unsubCoreEvent();
      this.unsubCoreEvent = null;
    }

    if (this.refreshDebounceTimeout) {
      clearTimeout(this.refreshDebounceTimeout);
      this.refreshDebounceTimeout = null;
    }

    this.listeners.clear();
    this._seenSessionIds.clear();
  }

  // ============================================================================
  // Internal
  // ============================================================================

  private tick(): void {
    if (this._state !== "idle") return;
    if (!this.focusManager) return;

    this.checkInactivity(this.focusManager.inactivitySeconds);
  }

  private async evaluateSwitch(): Promise<void> {
    this.transitionTo("checking_inactivity");

    try {
      await this.refreshSessions();

      const { cachedSessions: sessions, cachedActiveSessionId: activeSessionId } = this;

      if (sessions.length < 2 || !activeSessionId) {
        this.transitionTo("idle");
        return;
      }

      const activeSession = sessions.find((s) => s.id === activeSessionId);
      if (!activeSession || ATTENTION_STATES.has(activeSession.state)) {
        // Already on a session that needs attention — no switch needed
        this.transitionTo("idle");
        return;
      }

      const target = sessions.find(
        (s) =>
          s.id !== activeSessionId &&
          ATTENTION_STATES.has(s.state) &&
          !this._seenSessionIds.has(s.id)
      );

      if (!target) {
        this.transitionTo("idle");
        return;
      }

      this._toastState = {
        targetSessionId: target.id,
        targetSessionName: target.title || `Session ${target.id.slice(0, 8)}`,
        countdownSeconds: this.config.countdownSeconds,
      };
      this.transitionTo("showing_toast");
    } catch (err) {
      console.error("[SessionStateManager] evaluateSwitch error:", err);
      this.transitionTo("idle");
    }
  }

  private transitionTo(newState: SwitchState): void {
    this._state = newState;
    this.notifyListeners();
  }

  private notifyListeners(): void {
    const state = this._state;
    const toast = this.toastState;

    for (const listener of this.listeners) {
      try {
        listener(state, toast);
      } catch (err) {
        console.error("[SessionStateManager] Listener error:", err);
      }
    }
  }

  private refreshSessionsDebounced(): void {
    if (this.refreshDebounceTimeout) {
      clearTimeout(this.refreshDebounceTimeout);
    }
    this.refreshDebounceTimeout = setTimeout(() => {
      this.refreshSessions().catch((err) => {
        console.warn("[SessionStateManager] Debounced refresh failed:", err);
      });
    }, 100);
  }

  private async refreshSessions(): Promise<void> {
    if (this.refreshLock) return;
    this.refreshLock = true;

    try {
      const [sessions, activeSessionId] = await Promise.all([
        invoke<SessionInfo[]>("list_sessions"),
        invoke<string | null>("get_active_session"),
      ]);

      this.cachedSessions = sessions;
      this.cachedActiveSessionId = activeSessionId;
    } catch (err) {
      console.warn("[SessionStateManager] Failed to refresh sessions:", err);
    } finally {
      this.refreshLock = false;
    }
  }
}
