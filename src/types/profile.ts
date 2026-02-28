export interface ProfileInput {
  key: string;
  label: string;
  placeholder?: string;
  input_type: string;
  required: boolean;
  options?: string[];
  default?: string;
}

export interface WorkingDirConfig {
  type: "fixed" | "prompt" | "from_input";
  path?: string;
  key?: string;
}

export interface McpConfig {
  config_path?: string;
  servers?: Record<string, unknown>;
}

export interface Profile {
  id: string;
  name: string;
  description?: string;
  version: number;
  working_directory?: WorkingDirConfig;
  prompt_template?: string;
  auto_execute: boolean;
  mcp_servers?: McpConfig;
  skills?: string[];
  allowed_tools?: string[];
  model?: string;
  system_prompt?: string;
  disabled_mcps?: string[];
  system_prompt_file?: string;
  inputs: ProfileInput[];
  tags: string[];
}

export interface ProfileLaunchRequest {
  profile_id: string;
  input_values: Record<string, string>;
  working_directory?: string;
}

export interface McpServerEntry {
  name: string;
  server_type: string;
}

export interface SystemPromptEntry {
  name: string;
  preview: string;
}

export interface SkillInfo {
  name: string;
  source_path: string;
  is_active: boolean;
  group: string;
}
