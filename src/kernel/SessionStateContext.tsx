/**
 * SessionStateContext - React context for SessionStateManager
 *
 * Provides the useSessionState() hook for extensions to access the
 * session switching state machine through proper React state management.
 */

import React, { createContext, useContext, useEffect, useState, useCallback } from "react";
import type { ISessionStateManager, SwitchState, ToastState } from "../types/kernel";

interface SessionStateContextValue {
  /** Current state machine state */
  switchState: SwitchState;
  /** Current toast state (if showing) */
  toastState: ToastState | null;
  /** Whether a toast is currently showing */
  isToastShowing: boolean;
  /** Complete the switch (toast countdown finished) */
  completeSwitch: () => Promise<void>;
  /** Dismiss the toast and mark session as seen */
  dismissToast: () => void;
}

const SessionStateContext = createContext<SessionStateContextValue | null>(null);

interface SessionStateProviderProps {
  manager: ISessionStateManager;
  children: React.ReactNode;
}

export function SessionStateProvider({ manager, children }: SessionStateProviderProps) {
  const [switchState, setSwitchState] = useState<SwitchState>(manager.state);
  const [toastState, setToastState] = useState<ToastState | null>(manager.toastState);

  useEffect(() => {
    return manager.subscribe((newState, newToast) => {
      setSwitchState(newState);
      setToastState(newToast);
    });
  }, [manager]);

  const completeSwitch = useCallback(async () => {
    await manager.completeSwitch();
  }, [manager]);

  const dismissToast = useCallback(() => {
    manager.dismissToast();
  }, [manager]);

  const value: SessionStateContextValue = {
    switchState,
    toastState,
    isToastShowing: switchState === "showing_toast" && toastState !== null,
    completeSwitch,
    dismissToast,
  };

  return (
    <SessionStateContext.Provider value={value}>{children}</SessionStateContext.Provider>
  );
}

/**
 * Hook to access session state machine and operations.
 *
 * @example
 * const { isToastShowing, toastState, completeSwitch, dismissToast } = useSessionState();
 */
export function useSessionState(): SessionStateContextValue {
  const context = useContext(SessionStateContext);
  if (!context) {
    throw new Error("useSessionState must be used within a SessionStateProvider");
  }
  return context;
}
