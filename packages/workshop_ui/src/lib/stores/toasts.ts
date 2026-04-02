import { writable, derived } from 'svelte/store';

export type ToastType = 'info' | 'warn' | 'error';

export interface Toast {
  id: number;
  message: string;
  type: ToastType;
  duration: number;
}

const MAX_VISIBLE = 3;
let nextToastId = 1;

const _toasts = writable<Toast[]>([]);

export const toasts = derived(_toasts, ($t) => $t);

export function addToast(message: string, type: ToastType = 'info', duration = 3000): void {
  const id = nextToastId++;
  _toasts.update((list) => {
    const next = [...list, { id, message, type, duration }];
    // FIFO eviction if over max
    while (next.length > MAX_VISIBLE) next.shift();
    return next;
  });
  setTimeout(() => removeToast(id), duration);
}

export function removeToast(id: number): void {
  _toasts.update((list) => list.filter((t) => t.id !== id));
}
