import type { ToolCell } from '$lib/types';

export interface AllowedPrompt {
  tool: string;
  prompt: string;
}

export function parseAllowedPrompts(raw: unknown): AllowedPrompt[] {
  if (!Array.isArray(raw)) return [];
  return raw.filter(
    (p): p is AllowedPrompt =>
      typeof p === 'object' &&
      p !== null &&
      typeof (p as Record<string, unknown>).tool === 'string' &&
      typeof (p as Record<string, unknown>).prompt === 'string'
  );
}

export function getPlanContent(t: ToolCell): string | null {
  if (typeof t.input.plan === 'string' && t.input.plan.length > 0) {
    return t.input.plan;
  }
  return null;
}

export function parseStatusText(output: string | undefined): string | null {
  if (!output) return null;
  const lower = output.toLowerCase();
  if (lower.includes('approved') || lower.includes('accepted')) return 'APPROVED';
  if (lower.includes('rejected') || lower.includes('denied')) return 'REJECTED';
  if (lower.includes('changes requested')) return 'CHANGES REQUESTED';
  return null;
}
