<script lang="ts">
  import type { ToolCell } from '$lib/types';
  import { getContext } from 'svelte';
  import { setPaneViewMode } from '$lib/stores/layout';

  const paneCtx = getContext<{ readonly id: string }>('paneId');

  interface Props {
    tool: ToolCell;
  }

  let { tool }: Props = $props();

  let showRaw = $state(false);
  let activeTab = $state(0);

  // Parse AskUserQuestion structured input
  interface QuestionOption {
    label: string;
    description?: string;
  }

  interface StructuredQuestion {
    question: string;
    header?: string;
    options: QuestionOption[];
    multiSelect?: boolean;
  }

  const questions: StructuredQuestion[] = $derived.by(() => {
    const input = tool.input;
    // AskUserQuestion sends { questions: [...] }
    if (Array.isArray(input.questions)) {
      return input.questions as StructuredQuestion[];
    }
    // Fallback: single question shape
    if (typeof input.question === 'string') {
      return [
        {
          question: input.question as string,
          header: input.header as string | undefined,
          options: Array.isArray(input.options) ? (input.options as QuestionOption[]) : [],
          multiSelect: input.multiSelect as boolean | undefined
        }
      ];
    }
    return [];
  });

  const isResolved = $derived(!!tool.output);
  const isPending = $derived(!tool.output);

  // Extract answer values from tool output.
  // Format: User has answered your questions: "question"="answer", ...
  const answerValues: string[] = $derived.by(() => {
    if (!tool.output) return [];
    const matches = [...tool.output.matchAll(/="([^"]*)"/g)];
    return matches.map((m) => m[1]);
  });

  // Per-question answer analysis.
  // answerValues[i] is the positional answer for questions[i].
  // For each question, check whether the answer matches a known option label.
  interface QuestionAnswer {
    answer: string;
    matchedLabel: string | null;
    isOther: boolean;
  }

  const perQuestionAnswers: QuestionAnswer[] = $derived.by(() => {
    if (!isResolved) return [];
    return questions.map((q, i) => {
      const answer = i < answerValues.length ? answerValues[i] : '';
      const matchedLabel = q.options.find((o) => o.label === answer)?.label ?? null;
      return {
        answer,
        matchedLabel,
        isOther: answer.length > 0 && matchedLabel === null
      };
    });
  });

  // Per-question answered state for tab indicators.
  const questionAnswered: boolean[] = $derived.by(() => {
    if (!isResolved) return questions.map(() => false);
    return questions.map((_, i) => i < answerValues.length && answerValues[i].length > 0);
  });

  const isMultiQuestion = $derived(questions.length > 1);
</script>

<div class="question-card" class:pending={isPending} class:resolved={isResolved}>
  {#if showRaw}
    <!-- Raw view: show tool data for debugging -->
    <div class="raw-view">
      <div class="raw-header">
        <span class="raw-title">RAW — ASKUSERQUESTION</span>
        <button class="toggle-raw" onclick={() => (showRaw = false)} title="Show rendered">◆</button>
      </div>
      <div class="raw-field">
        <span class="raw-label">INPUT</span>
        <pre class="raw-value">{JSON.stringify(tool.input, null, 2)}</pre>
      </div>
      <div class="raw-field">
        <span class="raw-label">OUTPUT</span>
        <pre class="raw-value">{tool.output ?? '(none)'}</pre>
      </div>
      <div class="raw-field">
        <span class="raw-label">PARSED ANSWERS</span>
        <pre class="raw-value">{answerValues.length > 0 ? JSON.stringify(answerValues) : '(none)'}</pre>
      </div>
      <div class="raw-field">
        <span class="raw-label">PER-QUESTION</span>
        <pre class="raw-value">{perQuestionAnswers.length > 0
            ? JSON.stringify(perQuestionAnswers, null, 2)
            : '(none)'}</pre>
      </div>
      <div class="raw-field">
        <span class="raw-label">STATUS</span>
        <pre class="raw-value">{isResolved ? 'resolved' : 'pending'}</pre>
      </div>
    </div>
  {:else}
    <!-- Rendered view: structured Q/A card -->
    <div class="card-header">
      {#if isMultiQuestion}
        <div class="tab-row">
          {#each questions as q, qi}
            <button
              class="question-tab"
              class:tab-active={activeTab === qi}
              class:tab-answered={isResolved && questionAnswered[qi]}
              class:tab-unanswered={isResolved && !questionAnswered[qi]}
              onclick={() => (activeTab = qi)}
            >
              {q.header ?? `Q${qi + 1}`}
              {#if q.multiSelect}
                <span class="tab-multi">+</span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
      <button class="toggle-raw" onclick={() => (showRaw = true)} title="Show raw">◇</button>
    </div>

    <!-- Question content: single question shows directly, multi uses active tab -->
    {#each questions as q, qi}
      {#if !isMultiQuestion || activeTab === qi}
        <div class="question-block">
          {#if !isMultiQuestion && q.header}
            <div class="question-header">
              <span class="header-chip">{q.header}</span>
              {#if q.multiSelect}
                <span class="multi-badge">MULTI-SELECT</span>
              {/if}
            </div>
          {/if}

          <div class="question-text">{q.question}</div>

          {#if q.options.length > 0}
            {@const qa = perQuestionAnswers[qi]}
            <ol class="options-list">
              {#each q.options as opt, oi}
                <li
                  class="option"
                  class:first-option={!isResolved && oi === 0}
                  class:selected-option={isResolved && qa?.matchedLabel === opt.label}
                  class:unselected-option={isResolved && qa?.matchedLabel !== opt.label}
                >
                  <span class="option-number">{oi + 1}.</span>
                  <div class="option-body">
                    <span class="option-label">{opt.label}</span>
                    {#if opt.description}
                      <span class="option-desc">{opt.description}</span>
                    {/if}
                  </div>
                </li>
              {/each}
              <!-- "Other" option is always implicitly available -->
              <li
                class="option other-option"
                class:selected-option={qa?.isOther}
                class:unselected-option={isResolved && !qa?.isOther}
              >
                <span class="option-number">{q.options.length + 1}.</span>
                <div class="option-body">
                  <span class="option-label other-label">Other</span>
                  {#if qa?.isOther}
                    <span class="option-desc other-answer">{qa.answer}</span>
                  {:else}
                    <span class="option-desc">Custom text input</span>
                  {/if}
                </div>
              </li>
            </ol>
          {/if}
        </div>
      {/if}
    {/each}

    {#if isPending}
      <button class="pending-banner" onclick={() => setPaneViewMode(paneCtx.id, 'raw')}>
        <span class="pending-icon">⌨</span>
        <span class="pending-text">Switch to the Terminal view to answer this question</span>
      </button>
    {/if}
  {/if}
</div>

<style>
  .question-card {
    border: 1px solid var(--accent-600);
    border-radius: 4px;
    background: var(--surface-800);
    overflow: hidden;
    animation: card-on 0.3s ease-out;
  }

  .question-card.pending {
    border-color: var(--accent-500);
    box-shadow: var(--elevation-low);
  }

  .question-card.resolved {
    border-color: var(--surface-border);
    opacity: 0.85;
  }

  /* ── Card header with raw toggle ──────────── */

  .card-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 8px;
  }

  /* ── Question tabs (multi-question) ────────── */

  .tab-row {
    display: flex;
    gap: 3px;
  }

  .question-tab {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 8px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    font-family: inherit;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .question-tab:hover {
    background: var(--surface-600);
    border-color: var(--accent-600);
    color: var(--accent-400);
  }

  .question-tab.tab-active {
    background: var(--tint-active);
    border-color: var(--accent-500);
    color: var(--accent-400);
  }

  .question-tab.tab-answered {
    border-color: var(--status-green);
    color: var(--status-green-text);
  }

  .question-tab.tab-answered.tab-active {
    background: var(--tint-active);
  }

  .question-tab.tab-unanswered {
    opacity: 0.5;
  }

  .tab-multi {
    font-size: 8px;
    opacity: 0.6;
  }

  .toggle-raw {
    background: none;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 3px;
    opacity: 0.3;
    transition: all 0.15s ease;
  }

  .question-card:hover .toggle-raw {
    opacity: 0.8;
  }

  .toggle-raw:hover {
    background: var(--surface-500);
    color: var(--accent-400);
  }

  /* ── Raw view ─────────────────────────────── */

  .raw-view {
    padding: 0;
  }

  .raw-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-title {
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--accent-400);
  }

  .raw-field {
    padding: 6px 10px;
    border-bottom: 1px solid var(--surface-border);
  }

  .raw-field:last-child {
    border-bottom: none;
  }

  .raw-label {
    display: block;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.15em;
    color: var(--text-muted);
    margin-bottom: 2px;
  }

  .raw-value {
    margin: 0;
    white-space: pre-wrap;
    word-break: break-all;
    font-family: inherit;
    font-size: 10px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-height: 200px;
    overflow-y: auto;
  }

  /* ── Question blocks ──────────────────────── */

  .question-block {
    padding: 12px 14px;
  }

  .question-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .header-chip {
    display: inline-block;
    padding: 2px 8px;
    background: var(--tint-active);
    border: 1px solid var(--accent-600);
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--accent-400);
  }

  .multi-badge {
    font-size: 8px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.15em;
    color: var(--text-muted);
    padding: 1px 6px;
    border: 1px solid var(--surface-border);
    border-radius: 2px;
  }

  .question-text {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-primary);
    margin-bottom: 10px;
  }

  .options-list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .option {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 6px 10px;
    border-radius: 3px;
    background: var(--surface-700);
    border: 1px solid transparent;
    transition: all 0.15s ease;
  }

  .option.first-option {
    border-color: var(--accent-600);
    background: var(--tint-active);
  }

  .option-number {
    flex-shrink: 0;
    min-width: 16px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    text-align: right;
    line-height: 1.4;
  }

  .first-option .option-number {
    color: var(--accent-400);
  }

  .option-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .option-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-primary);
    line-height: 1.4;
  }

  .first-option .option-label {
    color: var(--accent-400);
  }

  .option-desc {
    font-size: 10px;
    color: var(--text-muted);
    line-height: 1.4;
  }

  /* ── Resolved selection states ─────────────── */

  .option.selected-option {
    border-color: var(--accent-600);
    background: var(--tint-active);
  }

  .option.selected-option .option-number {
    color: var(--accent-400);
  }

  .option.selected-option .option-label {
    color: var(--accent-400);
  }

  .option.unselected-option {
    opacity: 0.4;
    border-color: transparent;
  }

  .other-option {
    opacity: 0.5;
  }

  .other-option.selected-option {
    opacity: 1;
  }

  .other-option.unselected-option {
    opacity: 0.4;
  }

  .other-label {
    font-style: italic;
  }

  .other-answer {
    color: var(--text-primary);
    font-weight: 500;
    font-style: italic;
  }

  /* ── Pending banner ─────────────────────────── */

  .pending-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border: none;
    border-top: 1px solid var(--accent-600);
    border-radius: 0;
    background: var(--tint-active);
    width: 100%;
    cursor: pointer;
    font-family: inherit;
    transition: background 0.15s ease;
  }

  .pending-banner:hover {
    background: var(--accent-600);
  }

  .pending-icon {
    font-size: 12px;
    flex-shrink: 0;
  }

  .pending-text {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--accent-400);
  }

  @keyframes card-on {
    0% {
      opacity: 0;
      filter: brightness(3);
    }
    30% {
      opacity: 0.5;
      filter: brightness(2);
    }
    60% {
      opacity: 0.8;
      filter: brightness(1.2);
    }
    100% {
      opacity: 1;
      filter: brightness(1);
    }
  }

  /* Mobile */
  @media (max-width: 639px) {
    .question-block {
      padding: 10px 12px;
    }

    .question-text {
      font-size: 11px;
    }

    .option {
      padding: 5px 8px;
    }

    .option-label {
      font-size: 10px;
    }

    .pending-text {
      font-size: 9px;
    }

    .toggle-raw {
      opacity: 0.6;
      padding: 4px 8px;
      font-size: 14px;
    }
  }

  /* Analog theme */
  :global([data-theme='analog']) .question-card {
    background-color: var(--surface-800);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-color: var(--surface-border);
  }

  :global([data-theme='analog']) .question-card {
    animation: ink-bleed 0.5s cubic-bezier(0.1, 0.9, 0.2, 1);
  }

  @keyframes ink-bleed {
    0% {
      opacity: 0;
      transform: scaleY(0.95);
    }
    100% {
      opacity: 1;
      transform: scaleY(1);
    }
  }
</style>
