/**
 * Pure operations on per-instance draft messages.
 *
 * All functions are side-effect-free — the store layer (`stores/drafts.ts`)
 * handles Svelte reactivity and localStorage persistence.
 */

export type DraftMap = Map<string, string>;

/** Deserialize a JSON string into a DraftMap. Returns empty map on null/corrupt input. */
export function deserializeDrafts(json: string | null): DraftMap {
  if (!json) return new Map();
  try {
    const obj = JSON.parse(json) as Record<string, string>;
    if (typeof obj !== 'object' || obj === null || Array.isArray(obj)) return new Map();
    const entries = Object.entries(obj).filter(([k, v]) => typeof k === 'string' && typeof v === 'string' && v !== '');
    return new Map(entries);
  } catch {
    return new Map();
  }
}

/** Serialize a DraftMap to a JSON string. */
export function serializeDrafts(drafts: DraftMap): string {
  return JSON.stringify(Object.fromEntries(drafts));
}

/** Get the draft for an instance (empty string if none). */
export function getDraft(drafts: DraftMap, instanceId: string): string {
  return drafts.get(instanceId) ?? '';
}

/**
 * Return a new DraftMap with the draft for `instanceId` set to `text`.
 * If `text` is empty, the entry is removed (no stale empty keys).
 */
export function setDraft(drafts: DraftMap, instanceId: string, text: string): DraftMap {
  const next = new Map(drafts);
  if (text) {
    next.set(instanceId, text);
  } else {
    next.delete(instanceId);
  }
  return next;
}

/** Return a new DraftMap with the entry for `instanceId` removed. */
export function clearDraft(drafts: DraftMap, instanceId: string): DraftMap {
  return setDraft(drafts, instanceId, '');
}
