import { useEffect, useRef } from "react";
import { FrontendExtension } from "../../types/extension";
import { SLOTS } from "../../types/slots";
import { InactivityToast } from "./InactivityToast";
import { useSessionState } from "../../kernel/SessionStateContext";
import { useFocus } from "../../kernel/FocusContext";

/**
 * Inactivity Switch Extension
 *
 * When user is inactive on a session that doesn't need attention
 * while another session is in "your_turn" or "completed" state,
 * shows a countdown toast and auto-switches to that session.
 *
 * The SessionStateManager handles the check interval internally.
 * This extension only provides the overlay UI.
 */

function InactivityOverlay() {
  const { isToastShowing, toastState, completeSwitch, dismissToast } = useSessionState();
  const { recordActivity } = useFocus();
  const prevToastRef = useRef(false);

  // Dismiss toast on user activity (typing, clicking)
  useEffect(() => {
    const handleActivity = () => {
      if (isToastShowing) {
        dismissToast();
      }
      recordActivity();
    };

    window.addEventListener("terminal:activity", handleActivity);
    return () => window.removeEventListener("terminal:activity", handleActivity);
  }, [isToastShowing, dismissToast, recordActivity]);

  // Reset activity timer when toast closes
  useEffect(() => {
    if (!isToastShowing && prevToastRef.current) {
      recordActivity();
    }
    prevToastRef.current = isToastShowing;
  }, [isToastShowing, recordActivity]);

  if (!isToastShowing || !toastState) return null;

  return (
    <InactivityToast
      targetSessionName={toastState.targetSessionName}
      countdownSeconds={toastState.countdownSeconds}
      onComplete={completeSwitch}
      onCancel={dismissToast}
    />
  );
}

export function createInactivitySwitchExtension(): FrontendExtension {
  return {
    manifest: {
      id: "inactivity-switch",
      name: "Inactivity Switch",
      version: "0.2.0",
      description: "Auto-switch to your_turn sessions after inactivity",
    },

    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.OVERLAY, {
        id: "inactivity-overlay",
        component: InactivityOverlay,
        priority: 95,
        extensionId: "inactivity-switch",
      });
    },
  };
}
