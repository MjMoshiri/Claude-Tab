import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { McpServerEntry } from "../../types/profile";

interface McpPickerProps {
  disabledMcps: Set<string>;
  onDisabledChange: (disabled: Set<string>) => void;
}

export function McpPicker({ disabledMcps, onDisabledChange }: McpPickerProps) {
  const [servers, setServers] = useState<McpServerEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let mounted = true;
    invoke<McpServerEntry[]>("list_mcp_servers")
      .then((result) => {
        if (mounted) setServers(result);
      })
      .catch((err) => console.error("[McpPicker] Failed to load servers:", err))
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => {
      mounted = false;
    };
  }, []);

  const toggleServer = useCallback(
    (name: string) => {
      const next = new Set(disabledMcps);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      onDisabledChange(next);
    },
    [disabledMcps, onDisabledChange]
  );

  if (loading) {
    return <span className="profiles-field-hint">Loading MCP servers...</span>;
  }

  if (servers.length === 0) {
    return (
      <span className="profiles-field-hint">
        No MCP servers found
      </span>
    );
  }

  return (
    <div className="skill-picker">
      {servers.map((server) => (
        <label key={server.name} className="skill-picker-item">
          <input
            type="checkbox"
            checked={!disabledMcps.has(server.name)}
            onChange={() => toggleServer(server.name)}
          />
          <span>{server.name}</span>
          <span className="mcp-picker-type-badge">{server.server_type}</span>
        </label>
      ))}
    </div>
  );
}
