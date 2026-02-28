use async_trait::async_trait;
use claude_tabs_core::traits::extension::{
    ActivationContext, Extension, ExtensionError, ExtensionManifest,
};
use std::sync::Arc;
use tracing::info;

pub struct SystemNotifyExtension {
    manifest: ExtensionManifest,
}

impl SystemNotifyExtension {
    pub fn new() -> Self {
        Self {
            manifest: ExtensionManifest::new("system-notify", "System Notifications")
                .with_description("macOS notifications for permission requests"),
        }
    }
}

#[async_trait]
impl Extension for SystemNotifyExtension {
    fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    async fn activate(&mut self, ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
        let event_bus = ctx.event_bus.clone();

        let handler = Arc::new(move |event: &claude_tabs_core::event_bus::Event| {
            if event.topic == "session.state_changed" {
                if let Some(to) = event.payload.get("to").and_then(|v| v.as_str()) {
                    if to == "your_turn" || to == "completed" {
                        let body = if to == "completed" {
                            "A session has completed"
                        } else {
                            "A session needs your attention"
                        };
                        send_macos_notification("Claude Tabs", body);
                    }
                }
            }
        });

        event_bus.subscribe("session.state_changed", handler).await;

        Ok(())
    }

    async fn deactivate(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

fn send_macos_notification(title: &str, body: &str) {
    let script = format!(
        r#"display notification "{}" with title "{}""#,
        body.replace('"', r#"\""#),
        title.replace('"', r#"\""#)
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .spawn();
    info!(title = %title, body = %body, "Sent macOS notification");
}
