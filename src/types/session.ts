export interface SessionInfo {
  id: string;
  provider_id: string;
  state: string;
  title: string;
  working_directory: string | null;
  subtitle: string | null;
  tags: string[] | null;
  summary: string | null;
  user_set_title: boolean;
  generated_title: string | null;
  hidden: boolean;
  previous_session_id: string | null;
}

export interface CreateSessionRequest {
  provider_id: string;
  working_directory?: string;
  title?: string;
  resume_claude_session_id?: string;
  fork?: boolean;
  initial_prompt?: string;
  allowed_tools?: string[];
  model?: string;
  system_prompt?: string;
  system_prompt_file?: string;
  metadata?: Record<string, unknown>;
}

export interface WorktreeInfo {
  path: string;
  branch: string;
  repo_path: string;
}
