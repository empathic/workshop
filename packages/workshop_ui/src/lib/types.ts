// API Types - matches Rust backend

export interface Instance {
  id: string;
  name: string;
  custom_name?: string | null;
  command: string;
  kind: InstanceKind;
  working_dir: string;
  wrapper_port: number;
  running: boolean;
  created_at: string;
  session_id?: string;
  claude_state?: ClaudeState;
  claude_state_stale?: boolean; // True if terminal output is stale
  state_entered_at?: number; // Unix timestamp when current state started
}

export interface CreateInstanceRequest {
  name?: string;
  command?: string;
  working_dir?: string;
}

export interface CreateInstanceResponse {
  id: string;
  name: string;
  wrapper_port: number;
}

// Conversation types - the reactive document model

export type Role = 'User' | 'Assistant' | 'System' | 'Unknown' | 'AgentProgress' | 'Progress' | 'Skip';

export interface ConversationTurn {
  uuid?: string;
  role: Role;
  content: string;
  timestamp: string;
  tools: string[];
  tool_details?: Array<{
    name: string;
    input: Record<string, unknown>;
    category?: string;
    result?: string;
    is_error?: boolean;
  }>;
  thinking?: string; // Extended thinking content from Claude
  attributed_to?: { user_id: string; display_name: string };
  task_id?: number; // Structural task reference (from backend attribution)
  // For unknown entries
  unknown?: boolean;
  entry_type?: string;
  extra?: Record<string, unknown>;
  tool_result?: unknown;
  // For skip entries (hook_progress)
  skip?: boolean;
  // For agent progress entries
  agent_id?: string;
  agent_prompt?: string;
  agent_msg_role?: string;
  agent_tools?: Array<{ name: string; input?: Record<string, unknown> }>;
  // For progress entries (hook/agent)
  progress_type?: 'hook' | 'agent';
  hook_event?: string;
}

// Multi-user presence
export interface PresenceUser {
  user_id: string;
  display_name: string;
}

// Auth types
export interface AuthUser {
  id: string;
  username: string;
  display_name: string;
  is_admin: boolean;
}

export interface ConversationResponse {
  conversation_id?: string;
  turns: ConversationTurn[];
  error?: string;
}

export interface PollResponse {
  new_turns: ConversationTurn[];
  total_seen?: number;
  waiting?: boolean;
  error?: string;
}

// Instance kind — structured (conversation-capable) vs unstructured (terminal-only)
export type InstanceKind = { type: 'Structured'; provider: string } | { type: 'Unstructured'; label?: string | null };

// Claude state types (from server-side inference)

export type ClaudeState =
  | { type: 'Initializing' }
  | { type: 'Starting' }
  | { type: 'Idle' }
  | { type: 'Thinking' }
  | { type: 'Responding' }
  | { type: 'ToolExecuting'; tool: string }
  | { type: 'WaitingForInput'; prompt?: string };

// WebSocket message types

export type WsMessageType =
  | 'Output'
  | 'ConversationFull'
  | 'ConversationUpdate'
  | 'SessionAmbiguous'
  | 'StateChange'
  | 'Input'
  | 'SessionSelect'
  | 'Resize'
  | 'Refresh';

export interface WsOutputMessage {
  type: 'Output';
  data: string;
}

export interface WsConversationFullMessage {
  type: 'ConversationFull';
  turns: ConversationTurn[];
}

export interface WsConversationUpdateMessage {
  type: 'ConversationUpdate';
  turns: ConversationTurn[];
}

export interface SessionCandidate {
  session_id: string;
  started_at?: string;
  message_count: number;
  preview?: string;
}

export interface WsSessionAmbiguousMessage {
  type: 'SessionAmbiguous';
  candidates: SessionCandidate[];
}

export interface WsInputMessage {
  type: 'Input';
  data: string;
}

export interface WsSessionSelectMessage {
  type: 'SessionSelect';
  session_id: string;
}

export interface WsResizeMessage {
  type: 'Resize';
  rows: number;
  cols: number;
}

export interface WsRefreshMessage {
  type: 'Refresh';
}

export interface WsStateChangeMessage {
  type: 'StateChange';
  state: ClaudeState;
  stale?: boolean; // True if terminal output is stale - indicates lower confidence
}

export type WsMessage =
  | WsOutputMessage
  | WsConversationFullMessage
  | WsConversationUpdateMessage
  | WsSessionAmbiguousMessage
  | WsStateChangeMessage
  | WsInputMessage
  | WsSessionSelectMessage
  | WsResizeMessage
  | WsRefreshMessage;

// History / Conversation Summary types

export interface ConversationSummary {
  id: string;
  title: string | null;
  instance_id: string;
  created_at: number; // Unix timestamp
  updated_at: number; // Unix timestamp
  entry_count: number;
  is_public: boolean;
}

export interface ConversationEntry {
  id: number | null;
  conversation_id: string;
  entry_uuid: string;
  parent_uuid: string | null;
  entry_type: string;
  role: string | null;
  content: string | null;
  timestamp: string;
  raw_json: string;
  token_count: number | null;
  model: string | null;
}

export interface Conversation {
  id: string;
  session_id: string | null;
  instance_id: string;
  title: string | null;
  created_at: number;
  updated_at: number;
  is_public: boolean;
  is_deleted: boolean;
  metadata_json: string | null;
}

export interface Comment {
  id: number | null;
  conversation_id: string;
  entry_uuid: string | null;
  author: string;
  content: string;
  created_at: number;
  updated_at: number | null;
}

export interface Tag {
  id: number;
  name: string;
  color: string | null;
}

export interface EntryAttribution {
  entry_uuid: string;
  user_id: string;
  display_name: string;
  task_id?: number;
}

export interface ConversationWithEntries {
  conversation: Conversation;
  entries: ConversationEntry[];
  comments: Comment[];
  tags: Tag[];
  attributions?: EntryAttribution[];
}

// Observable-style reactive cell model for tool calls

export interface ToolCell {
  id: string;
  name: string;
  input: Record<string, unknown>;
  output?: string;
  is_error?: boolean;
  status: 'pending' | 'running' | 'complete' | 'error';
  timestamp: string;
  // For re-execution
  canRerun: boolean;
  currentState?: string; // Live comparison with current file state
}

export interface NotebookCell {
  id: string;
  type: 'user' | 'assistant' | 'tool' | 'system' | 'unknown' | 'agent' | 'progress';
  content: string;
  timestamp: string;
  // For tool cells
  toolCells?: ToolCell[];
  // Extended thinking content from Claude
  thinking?: string;
  // Metadata
  collapsed?: boolean;
  // Multi-user attribution
  attributed_to?: { user_id: string; display_name: string };
  // Structural task reference (from backend attribution)
  task_id?: number;
  // For unknown entries - raw entry type from the server
  entryType?: string;
  // Extra data for unknown entries
  extra?: Record<string, unknown>;
  // For agent progress entries
  agentId?: string;
  agentPrompt?: string;
  agentMsgRole?: string;
  // For progress entries
  progressType?: 'hook' | 'agent';
  hookEvent?: string;
  // Agent progress absorbed during tool groups (sub-agent activity log)
  agentLog?: Array<{
    content: string;
    agentId?: string;
    role?: string;
    tools?: Array<{ name: string; input?: Record<string, unknown> }>;
  }>;
}

// Pagination and search types

export interface PaginatedResponse<T> {
  items: T[];
  total: number;
  page: number;
  per_page: number;
  total_pages: number;
}

export interface SearchMatchEntry {
  entry_uuid: string;
  role: string | null;
  snippet: string;
  timestamp: string;
}

export interface SearchResultConversation {
  id: string;
  title: string | null;
  instance_id: string;
  created_at: number;
  updated_at: number;
  entry_count: number;
  match_count: number;
  matches: SearchMatchEntry[];
}

// Todo queue item (per-instance task queue)
/** @deprecated Use Task instead */
export interface TodoItem {
  id: string;
  text: string;
  createdAt: number;
  status: 'pending' | 'sent'; // Legacy: kept for localStorage migration compat
}

// Task dispatch record — one per send to an instance
export interface TaskDispatch {
  id: number;
  task_id: number;
  instance_id: string;
  sent_text: string;
  conversation_id: string | null;
  sent_at: number;
}

// Server-backed task
export interface Task {
  id: number;
  uuid: string;
  title: string;
  body: string | null;
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled';
  priority: number;
  instance_id: string | null;
  creator_id: string | null;
  creator_name: string;
  sort_order: number;
  created_at: number;
  updated_at: number;
  completed_at: number | null;
  sent_text: string | null;
  conversation_id: string | null;
  tags: Tag[];
  dispatches?: TaskDispatch[];
}

export interface CreateTaskRequest {
  title: string;
  body?: string;
  status?: string;
  priority?: number;
  instance_id?: string;
  tags?: string[];
}

export interface UpdateTaskRequest {
  title?: string;
  body?: string;
  status?: string;
  priority?: number;
  instance_id?: string;
  sort_order?: number;
  sent_text?: string;
  conversation_id?: string;
}

// Minimap visualization types
export interface MinimapSegment {
  id: string;
  type: 'user' | 'assistant';
  /** Normalized height (0-1) based on content length */
  heightRatio: number;
  /** Offset from top (0-1) */
  offsetRatio: number;
  /** Whether this segment has tool usage */
  hasTools: boolean;
  /** Timestamp for the segment */
  timestamp: Date;
}
