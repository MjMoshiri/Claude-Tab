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

export interface Profile {
  id: string;
  name: string;
  description?: string;
  version: number;
  working_directory?: WorkingDirConfig;
  prompt_template?: string;
  auto_execute: boolean;
  skills?: string[];
  allowed_tools?: string[];
  model?: string;
  system_prompt?: string;
  system_prompt_file?: string;
  inputs: ProfileInput[];
  tags: string[];
  is_default?: boolean;
}

export interface ProfileLaunchRequest {
  profile_id: string;
  input_values: Record<string, string>;
  working_directory?: string;
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
