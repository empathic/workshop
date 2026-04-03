/** Fuzzy match: returns matched character indices, or null if no match */
export function fuzzyMatch(pattern: string, str: string): number[] | null {
  if (!pattern) return [];

  const patternLower = pattern.toLowerCase();
  const strLower = str.toLowerCase();
  const indices: number[] = [];
  let patternIdx = 0;

  for (let i = 0; i < str.length && patternIdx < pattern.length; i++) {
    if (strLower[i] === patternLower[patternIdx]) {
      indices.push(i);
      patternIdx++;
    }
  }

  return patternIdx === pattern.length ? indices : null;
}

/** Score a fuzzy match (lower is better) */
export function fuzzyScore(pattern: string, str: string, indices: number[]): number {
  if (indices.length === 0) return 0;

  let score = 0;

  // Prefer matches at start of string
  if (indices[0] === 0) score -= 10;

  // Prefer consecutive matches
  for (let i = 1; i < indices.length; i++) {
    if (indices[i]! === indices[i - 1]! + 1) {
      score -= 5;
    } else {
      score += indices[i]! - indices[i - 1]!;
    }
  }

  // Prefer shorter strings
  score += str.length * 0.1;

  // Prefer exact case matches
  for (let i = 0; i < indices.length; i++) {
    if (str[indices[i]!] === pattern[i]) {
      score -= 1;
    }
  }

  return score;
}

/** Render name with highlighted match segments */
export function highlightMatches(name: string, indices: number[]): Array<{ text: string; highlight: boolean }> {
  if (indices.length === 0) {
    return [{ text: name, highlight: false }];
  }

  const parts: Array<{ text: string; highlight: boolean }> = [];
  const indexSet = new Set(indices);
  let currentPart = '';
  let currentHighlight = false;

  for (let i = 0; i < name.length; i++) {
    const isMatch = indexSet.has(i);
    if (isMatch !== currentHighlight && currentPart) {
      parts.push({ text: currentPart, highlight: currentHighlight });
      currentPart = '';
    }
    currentPart += name[i];
    currentHighlight = isMatch;
  }

  if (currentPart) {
    parts.push({ text: currentPart, highlight: currentHighlight });
  }

  return parts;
}
