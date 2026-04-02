/**
 * Server-backed Task Store
 *
 * Replaces the localStorage-based todoQueue with server-authoritative tasks.
 * Tasks are globally visible, taggable, and assignable to instances.
 */

import { writable, derived, get } from 'svelte/store';
import type { Task, TaskDispatch, CreateTaskRequest, UpdateTaskRequest } from '$lib/types';
import { api } from '$lib/utils/api';
import { currentInstanceId, selectInstance, onInstanceDelete } from './instances';
import { browser } from '$app/environment';

const API = '/api/tasks';

// =============================================================================
// Core Stores
// =============================================================================

export const tasks = writable<Task[]>([]);
export const tasksLoaded = writable(false);

// =============================================================================
// Task Panel UI State (formerly taskPanel.ts)
// =============================================================================

export const isTaskPanelOpen = writable<boolean>(false);

/** When set, the TaskPanel auto-expands this task for editing. */
export const focusedTaskId = writable<number | null>(null);

export function openTaskPanel(): void {
  isTaskPanelOpen.set(true);
}

export function closeTaskPanel(): void {
  isTaskPanelOpen.set(false);
  focusedTaskId.set(null);
}

export function toggleTaskPanel(): void {
  const open = get(isTaskPanelOpen);
  if (open) {
    closeTaskPanel();
  } else {
    openTaskPanel();
  }
}

/** Open the panel and jump to a specific task for editing. */
export function openTaskForEdit(taskId: number): void {
  focusedTaskId.set(taskId);
  isTaskPanelOpen.set(true);
}

// =============================================================================
// WebSocket Handler (called by ws-handlers.ts for real-time sync)
// =============================================================================

/** Idempotent upsert: updates if existing, inserts if new. */
export function handleTaskUpdate(task: Task): void {
  tasks.update(($t) => {
    const idx = $t.findIndex((t) => t.id === task.id);
    if (idx >= 0) {
      $t[idx] = task;
      return [...$t];
    }
    return [...$t, task];
  });
}

/** Remove a task by ID (idempotent — no-ops if already gone). */
export function handleTaskDeleted(taskId: number): void {
  tasks.update(($t) => $t.filter((t) => t.id !== taskId));
}

// =============================================================================
// Derived Stores
// =============================================================================

/** Pending tasks for the currently focused instance (drives TodoQueue bar) */
export const currentInstanceTasks = derived([tasks, currentInstanceId], ([$tasks, $id]) =>
  $id
    ? $tasks.filter((t) => t.instance_id === $id && t.status === 'pending').sort((a, b) => a.sort_order - b.sort_order)
    : []
);

/** Count of pending items for the current instance */
export const currentInstanceTaskCount = derived(currentInstanceTasks, ($t) => $t.length);

/** All pending tasks (for task panel) */
export const pendingTasks = derived(tasks, ($t) =>
  $t.filter((t) => t.status === 'pending').sort((a, b) => a.sort_order - b.sort_order)
);

// =============================================================================
// API Actions
// =============================================================================

/** Fetch all non-deleted tasks from the server. */
export async function fetchTasks(): Promise<void> {
  try {
    const response = await api(API);
    if (!response.ok) throw new Error(`Failed to fetch tasks: ${response.status}`);
    const data: Task[] = await response.json();
    tasks.set(data);
    tasksLoaded.set(true);
  } catch (e) {
    console.error('Failed to fetch tasks:', e);
  }
}

/** Create a new task via the API. */
export async function createTask(req: CreateTaskRequest): Promise<Task | null> {
  try {
    const response = await api(API, {
      method: 'POST',
      body: JSON.stringify(req)
    });
    if (!response.ok) throw new Error(`Failed to create task: ${response.status}`);
    const created: Task = await response.json();

    // Idempotent upsert: avoids duplicates if the WS broadcast arrives first
    tasks.update(($t) => {
      const idx = $t.findIndex((t) => t.id === created.id);
      if (idx >= 0) {
        $t[idx] = created;
        return [...$t];
      }
      return [...$t, created];
    });
    return created;
  } catch (e) {
    console.error('Failed to create task:', e);
    return null;
  }
}

/** Update a task via the API. */
export async function updateTask(id: number, req: UpdateTaskRequest): Promise<void> {
  try {
    // Optimistic update
    tasks.update(($t) =>
      $t.map((t) => {
        if (t.id !== id) return t;
        const updated = { ...t, updated_at: Math.floor(Date.now() / 1000) };
        if (req.title !== undefined) updated.title = req.title;
        if (req.body !== undefined) updated.body = req.body;
        if (req.status !== undefined) updated.status = req.status as Task['status'];
        if (req.priority !== undefined) updated.priority = req.priority;
        if (req.instance_id !== undefined) updated.instance_id = req.instance_id ?? null;
        if (req.sort_order !== undefined) updated.sort_order = req.sort_order;
        if (req.sent_text !== undefined) updated.sent_text = req.sent_text;
        if (req.conversation_id !== undefined) updated.conversation_id = req.conversation_id;
        return updated;
      })
    );

    const response = await api(`${API}/${id}`, {
      method: 'PATCH',
      body: JSON.stringify(req)
    });
    if (!response.ok) {
      // Revert on failure
      await fetchTasks();
      throw new Error(`Failed to update task: ${response.status}`);
    }
  } catch (e) {
    console.error('Failed to update task:', e);
  }
}

/** Soft-delete a task via the API. */
export async function deleteTask(id: number): Promise<void> {
  try {
    // Optimistic: remove from local store
    tasks.update(($t) => $t.filter((t) => t.id !== id));

    const response = await api(`${API}/${id}`, { method: 'DELETE' });
    if (!response.ok) {
      await fetchTasks();
      throw new Error(`Failed to delete task: ${response.status}`);
    }
  } catch (e) {
    console.error('Failed to delete task:', e);
  }
}

/** Create a dispatch record for a task (records that it was sent to an instance). */
export async function createDispatch(taskId: number, instanceId: string, sentText: string): Promise<void> {
  try {
    // Optimistic: append dispatch and transition status
    const now = Math.floor(Date.now() / 1000);
    const optimisticDispatch: TaskDispatch = {
      id: -1,
      task_id: taskId,
      instance_id: instanceId,
      sent_text: sentText,
      conversation_id: null,
      sent_at: now
    };
    tasks.update(($t) =>
      $t.map((t) => {
        if (t.id !== taskId) return t;
        return {
          ...t,
          status: t.status === 'pending' ? ('in_progress' as const) : t.status,
          dispatches: [...(t.dispatches ?? []), optimisticDispatch]
        };
      })
    );

    const response = await api(`${API}/${taskId}/dispatch`, {
      method: 'POST',
      body: JSON.stringify({ instance_id: instanceId, sent_text: sentText })
    });
    if (!response.ok) {
      await fetchTasks();
      throw new Error(`Failed to create dispatch: ${response.status}`);
    }
  } catch (e) {
    console.error('Failed to create dispatch:', e);
  }
}

/** Add a tag to a task. */
export async function addTaskTag(id: number, tagName: string): Promise<void> {
  try {
    const response = await api(`${API}/${id}/tags`, {
      method: 'POST',
      body: JSON.stringify({ tag: tagName })
    });
    if (!response.ok) throw new Error(`Failed to add tag: ${response.status}`);
    // Refresh to get updated tag list
    await fetchTasks();
  } catch (e) {
    console.error('Failed to add task tag:', e);
  }
}

/** Remove a tag from a task. */
export async function removeTaskTag(id: number, tagId: number): Promise<void> {
  try {
    const response = await api(`${API}/${id}/tags/${tagId}`, { method: 'DELETE' });
    if (!response.ok) throw new Error(`Failed to remove tag: ${response.status}`);
    await fetchTasks();
  } catch (e) {
    console.error('Failed to remove task tag:', e);
  }
}

// =============================================================================
// Staged Task — loads a task into MessageInput for review before sending
// =============================================================================

/** The task currently staged in the message input, if any. */
export const stagedTask = writable<Task | null>(null);

/**
 * Stage a task: switch to its instance, load its text into the input,
 * and close the task panel. The user reviews and presses Enter to send.
 */
export function stageTask(id: number): void {
  const $tasks = get(tasks);
  const task = $tasks.find((t) => t.id === id);
  if (!task) return;

  // Switch to the assigned instance
  if (task.instance_id) {
    selectInstance(task.instance_id);
  }

  stagedTask.set(task);
}

/** Clear the staged task without sending. */
export function clearStagedTask(): void {
  stagedTask.set(null);
}

/** Record a dispatch after the user submits the staged task through MessageInput. */
export async function commitStagedTask(finalText: string): Promise<void> {
  const task = get(stagedTask);
  if (!task) return;
  stagedTask.set(null);
  const instanceId = task.instance_id ?? get(currentInstanceId) ?? 'unknown';
  await createDispatch(task.id, instanceId, finalText);
}

// =============================================================================
// Convenience Actions (used by UI components)
// =============================================================================

/** Quick-add a task from MessageInput (replaces addTodoItem). */
export async function quickAddTask(instanceId: string, text: string): Promise<void> {
  await createTask({
    title: text,
    instance_id: instanceId
  });
}

/** Stage the next pending task for an instance into MessageInput for review. */
export function sendNextTask(instanceId: string): void {
  const $tasks = get(tasks);
  const next = $tasks
    .filter((t) => t.instance_id === instanceId && t.status === 'pending')
    .sort((a, b) => a.sort_order - b.sort_order)[0];

  if (!next) return;
  stageTask(next.id);
}

/** Reorder a task by computing a new sort_order midpoint. */
export async function reorderTask(instanceId: string, taskId: number, newIndex: number): Promise<void> {
  const $tasks = get(tasks);
  const queue = $tasks
    .filter((t) => t.instance_id === instanceId && t.status === 'pending')
    .sort((a, b) => a.sort_order - b.sort_order);

  if (queue.length < 2) return;

  let newSortOrder: number;
  if (newIndex === 0) {
    newSortOrder = queue[0].sort_order - 1.0;
  } else if (newIndex >= queue.length - 1) {
    newSortOrder = queue[queue.length - 1].sort_order + 1.0;
  } else {
    // Midpoint between neighbors (excluding the item being moved)
    const filtered = queue.filter((t) => t.id !== taskId);
    const before = filtered[newIndex - 1]?.sort_order ?? 0;
    const after = filtered[newIndex]?.sort_order ?? before + 2;
    newSortOrder = (before + after) / 2;
  }

  await updateTask(taskId, { sort_order: newSortOrder });
}

/** Clear all pending tasks for an instance. */
export async function clearInstanceTasks(instanceId: string): Promise<void> {
  const $tasks = get(tasks);
  const toDelete = $tasks.filter((t) => t.instance_id === instanceId && t.status === 'pending');
  for (const t of toDelete) {
    await deleteTask(t.id);
  }
}

/** Get count of pending tasks for a specific instance (non-reactive helper). */
export function getTaskCount(instanceId: string): number {
  const $tasks = get(tasks);
  return $tasks.filter((t) => t.instance_id === instanceId && t.status === 'pending').length;
}

// =============================================================================
// One-time localStorage Migration
// =============================================================================

/** Migrate any existing localStorage todo items to server-backed tasks. */
export async function migrateFromLocalStorage(): Promise<void> {
  if (!browser) return;

  const raw = localStorage.getItem('workshop_todo_queues');
  if (!raw) return;

  try {
    const queues: Record<string, Array<{ id: string; text: string; createdAt: number; status: string }>> = JSON.parse(
      raw
    );

    const items: Array<{ title: string; instance_id?: string; created_at?: number }> = [];
    for (const [instanceId, queue] of Object.entries(queues)) {
      for (const item of queue) {
        if (item.status === 'pending') {
          items.push({
            title: item.text,
            instance_id: instanceId,
            created_at: Math.floor(item.createdAt / 1000)
          });
        }
      }
    }

    if (items.length === 0) {
      localStorage.removeItem('workshop_todo_queues');
      return;
    }

    const response = await api(`${API}/migrate`, {
      method: 'POST',
      body: JSON.stringify(items)
    });

    if (response.ok) {
      localStorage.removeItem('workshop_todo_queues');
      // Refresh to get the migrated tasks
      await fetchTasks();
      console.log(`Migrated ${items.length} tasks from localStorage`);
    }
  } catch (e) {
    console.error('Failed to migrate tasks from localStorage:', e);
  }
}

// =============================================================================
// Instance delete cleanup
// =============================================================================

onInstanceDelete(async (instanceId: string) => {
  // Unassign tasks from the deleted instance (they become orphaned, not deleted)
  const $tasks = get(tasks);
  const affected = $tasks.filter((t) => t.instance_id === instanceId);
  for (const t of affected) {
    await updateTask(t.id, { instance_id: undefined });
  }
});
