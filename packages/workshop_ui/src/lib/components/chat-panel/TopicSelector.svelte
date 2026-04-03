<script lang="ts">
  interface Props {
    activeTopic: string | null;
    currentTopics: Array<{ topic: string; message_count: number }>;
    ontopicselect: (topic: string | null) => void;
  }

  let { activeTopic, currentTopics, ontopicselect }: Props = $props();

  let topicDropdownOpen = $state(false);
  let showNewTopicInput = $state(false);
  let topicInputValue = $state('');

  const topicLabel = $derived(activeTopic ?? 'All topics');

  function handleTopicSelect(topic: string | null) {
    ontopicselect(topic);
    topicDropdownOpen = false;
    showNewTopicInput = false;
  }

  function handleNewTopic() {
    const t = topicInputValue.trim();
    if (t) {
      ontopicselect(t);
      topicInputValue = '';
    }
    showNewTopicInput = false;
    topicDropdownOpen = false;
  }

  function handleNewTopicKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleNewTopic();
    }
    if (e.key === 'Escape') {
      showNewTopicInput = false;
      topicDropdownOpen = false;
    }
  }
</script>

<div class="topic-bar">
  <div class="topic-pill-area">
    <button
      class="topic-pill"
      class:has-topic={activeTopic !== null}
      onclick={() => {
        topicDropdownOpen = !topicDropdownOpen;
      }}
    >
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="12" height="12">
        <path d="M4 9h16M4 15h16M10 3l-2 18M16 3l-2 18" />
      </svg>
      <span>{topicLabel}</span>
      <svg
        class="chevron"
        class:open={topicDropdownOpen}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        width="10"
        height="10"
      >
        <path d="M6 9l6 6 6-6" />
      </svg>
    </button>

    {#if activeTopic !== null}
      <button class="topic-clear" onclick={() => handleTopicSelect(null)} title="Show all topics">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" width="10" height="10">
          <path d="M18 6L6 18M6 6l12 12" />
        </svg>
      </button>
    {/if}
  </div>

  {#if topicDropdownOpen}
    <div class="topic-dropdown">
      <button class="topic-option" class:active={activeTopic === null} onclick={() => handleTopicSelect(null)}>
        All topics
      </button>
      {#each currentTopics as t}
        <button class="topic-option" class:active={activeTopic === t.topic} onclick={() => handleTopicSelect(t.topic)}>
          <span class="topic-name"># {t.topic}</span>
          <span class="topic-meta">{t.message_count} msg</span>
        </button>
      {/each}
      {#if showNewTopicInput}
        <div class="new-topic-input">
          <input
            type="text"
            bind:value={topicInputValue}
            onkeydown={handleNewTopicKeydown}
            placeholder="Topic name..."
            class="topic-name-input"
          />
        </div>
      {:else}
        <button
          class="topic-option new-topic"
          onclick={() => {
            showNewTopicInput = true;
          }}
        >
          + New topic
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .topic-bar {
    display: flex;
    align-items: center;
    padding: 6px 14px;
    border-bottom: 1px solid var(--surface-border);
    background: var(--surface-800);
    flex-shrink: 0;
    position: relative;
  }

  .topic-pill-area {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .topic-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    background: var(--tint-active);
    border: 1px solid var(--tint-focus);
    border-radius: 3px;
    color: var(--text-secondary);
    font-size: 10px;
    font-family: inherit;
    font-weight: 500;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .topic-pill:hover {
    background: var(--tint-active-strong);
    border-color: var(--tint-selection);
    color: var(--chrome-accent-400);
  }

  .topic-pill.has-topic {
    background: var(--tint-active-strong);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .topic-pill .chevron {
    transition: transform 0.15s ease;
  }

  .topic-pill .chevron.open {
    transform: rotate(180deg);
  }

  .topic-clear {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 2px;
    color: var(--text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .topic-clear:hover {
    color: var(--chrome-accent-400);
    background: var(--tint-active-strong);
  }

  /* Topic dropdown */
  .topic-dropdown {
    position: absolute;
    top: 100%;
    left: 14px;
    right: 14px;
    z-index: 10;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--shadow-dropdown);
    padding: 4px 0;
    max-height: 200px;
    overflow-y: auto;
    animation: ctx-pop 0.12s ease-out;
  }

  @keyframes ctx-pop {
    from {
      opacity: 0;
      transform: scale(0.95);
    }
    to {
      opacity: 1;
      transform: scale(1);
    }
  }

  .topic-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 6px 12px;
    background: transparent;
    border: none;
    color: var(--text-secondary);
    font-size: 11px;
    font-family: inherit;
    letter-spacing: 0.03em;
    text-align: left;
    cursor: pointer;
    transition: all 0.1s ease;
  }

  .topic-option:hover {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
  }

  .topic-option.active {
    color: var(--chrome-accent-400);
    background: var(--tint-active);
  }

  .topic-option.new-topic {
    color: var(--text-muted);
    border-top: 1px solid var(--surface-border);
    margin-top: 2px;
    padding-top: 8px;
  }

  .topic-name {
    font-weight: 500;
  }

  .topic-meta {
    font-size: 9px;
    color: var(--text-muted);
  }

  .new-topic-input {
    padding: 4px 8px;
    border-top: 1px solid var(--surface-border);
    margin-top: 2px;
  }

  .topic-name-input {
    width: 100%;
    padding: 4px 6px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    color: var(--text-primary);
    font-size: 11px;
    font-family: inherit;
    outline: none;
  }

  .topic-name-input:focus {
    border-color: var(--chrome-accent-600);
  }
</style>
