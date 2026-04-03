<script lang="ts">
  import { sendMessage, connectionStatus, hasPendingInput, reconnect, shutdownReason } from '$lib/stores/websocket';
  import { isActive, isStarting, isActiveForInstance, isStartingForInstance } from '$lib/stores/claude';
  import { currentInstanceId } from '$lib/stores/instances';
  import { quickAddTask, stagedTask, clearStagedTask, commitStagedTask } from '$lib/stores/tasks';
  import { getDraft, setDraft } from '$lib/stores/drafts';
  import { voiceBackendOverride } from '$lib/stores/metrics';
  import { api } from '$lib/utils/api';
  import type { PaginatedResponse, SearchResultConversation } from '$lib/types';
  import { onMount, onDestroy, untrack } from 'svelte';
  import { detectVoiceBackend, createVoiceSession, type VoiceSession } from '$lib/utils/voice';
  import VoiceVisualizer from './VoiceVisualizer.svelte';
  import HistorySearchOverlay from './HistorySearchOverlay.svelte';

  interface Props {
    instanceId?: string;
  }

  let { instanceId: propInstanceId }: Props = $props();

  // Snapshot outside reactive tracking — component lifetime is tied to one instance
  const boundInstanceId = untrack(() => propInstanceId);

  // Per-instance stores when bound to a specific instance
  const _isStarting = boundInstanceId ? isStartingForInstance(boundInstanceId) : null;
  const _isActive = boundInstanceId ? isActiveForInstance(boundInstanceId) : null;

  // Bridge per-instance stores into $state
  let startingVal = $state(false);
  let activeVal = $state(false);
  const _unsubs: (() => void)[] = [];
  if (_isStarting)
    _unsubs.push(
      _isStarting.subscribe((v) => {
        startingVal = v;
      })
    );
  if (_isActive)
    _unsubs.push(
      _isActive.subscribe((v) => {
        activeVal = v;
      })
    );
  onDestroy(() => {
    for (const unsub of _unsubs) unsub();
  });

  // Use per-instance values when bound, otherwise fall back to global
  let isInstanceStarting = $derived(_isStarting ? startingVal : $isStarting);
  let isInstanceActive = $derived(_isActive ? activeVal : $isActive);

  const effectiveInstanceId = $derived(boundInstanceId ?? $currentInstanceId);

  // Restore draft from the persisted store (survives instance switches & reloads)
  const initialInstanceId = untrack(() => effectiveInstanceId);
  let message = $state(initialInstanceId ? getDraft(initialInstanceId) : '');
  let inputEl: HTMLTextAreaElement;

  let isDisconnected = $derived($connectionStatus === 'disconnected' || $connectionStatus === 'error');
  let isReconnecting = $derived($connectionStatus === 'connecting' || $connectionStatus === 'reconnecting');
  let isServerGone = $derived($connectionStatus === 'server_gone');
  let showBanner = $derived(isDisconnected || isReconnecting || isServerGone || $hasPendingInput);
  let showStartupBanner = $derived(isInstanceStarting);

  // Queue flash confirmation
  let queueFlash = $state(false);

  // CTRL+R history search — discriminated state makes impossible states impossible
  type HistorySearchMode = { mode: 'closed' } | { mode: 'open'; savedMessage: string };
  let historySearch = $state<HistorySearchMode>({ mode: 'closed' });
  let hsResults = $state<SearchResultConversation[]>([]);
  let hsLoading = $state(false);
  let hsSelectedIndex = $state(0);
  let hsSeq = 0;

  async function hsSearch(query: string): Promise<void> {
    const seq = ++hsSeq;
    hsLoading = true;
    try {
      const response = await api(
        `/api/conversations/search?q=${encodeURIComponent(query)}&role=user&per_page=10`
      );
      if (seq !== hsSeq) return;
      if (!response.ok) throw new Error(`Search failed: ${response.statusText}`);
      const data: PaginatedResponse<SearchResultConversation> = await response.json();
      if (seq !== hsSeq) return;
      hsResults = data.items;
      hsSelectedIndex = 0;
    } catch (e) {
      if (seq !== hsSeq) return;
      console.error('History search failed:', e);
      hsResults = [];
    } finally {
      if (seq === hsSeq) hsLoading = false;
    }
  }

  function hsClear(): void {
    hsSeq++;
    hsResults = [];
    hsLoading = false;
    hsSelectedIndex = 0;
  }

  // Debounced search driven by textarea content while overlay is open
  $effect(() => {
    if (historySearch.mode !== 'open') return;
    const q = message.trim();
    if (!q) {
      hsClear();
      return;
    }
    const timer = setTimeout(() => {
      hsSearch(q);
    }, 150);
    return () => clearTimeout(timer);
  });

  // Watch for staged tasks — populate input when one arrives
  $effect(() => {
    const task = $stagedTask;
    if (task) {
      message = task.body || task.title;
      // Focus the input so the user can review and send
      requestAnimationFrame(() => {
        inputEl?.focus();
        if (inputEl) autoResize(inputEl);
      });
    }
  });

  // Voice input state
  let speechSupported = $state(false);
  let isListening = $state(false);
  let isTranscribing = $state(false);
  let voiceLevel = $state(0);
  let voiceSession = $state<VoiceSession | null>(null);
  // Track the message content before voice input started, so we can append interim results
  let messageBeforeVoice = '';
  // Shared frequency buffer for visualizer — written by voice analyser, read by canvas RAF
  const frequencyBuffer = new Uint8Array(256);

  // Voice mode indicators
  let isHybrid = $derived(voiceSession?.backend === 'hybrid');
  let isPromptApi = $derived(voiceSession?.backend === 'prompt-api');
  let showDraftBanner = $derived(isListening && isHybrid);
  let showCorrectingBanner = $derived(isTranscribing && (isHybrid || isPromptApi));
  let showRecordingBanner = $derived(isListening && isPromptApi);

  function voiceCallbacks() {
    return {
      onInterim(text: string) {
        const separator = messageBeforeVoice && !messageBeforeVoice.endsWith(' ') ? ' ' : '';
        message = messageBeforeVoice + separator + text;
      },
      onFinal(text: string) {
        const separator = messageBeforeVoice && !messageBeforeVoice.endsWith(' ') ? ' ' : '';
        message = messageBeforeVoice + separator + text;
        messageBeforeVoice = message;
      },
      onError(err: string) {
        console.error('Voice input error:', err);
      },
      onStateChange(state: 'listening' | 'transcribing' | 'idle') {
        isListening = state === 'listening';
        isTranscribing = state === 'transcribing';
        if (state === 'idle') voiceLevel = 0;
      },
      onVolumeChange(level: number) {
        voiceLevel = level;
      },
      onFrequencyData(data: Uint8Array) {
        frequencyBuffer.set(data);
      }
    };
  }

  function initVoiceSession() {
    voiceSession?.destroy();
    voiceSession = null;
    detectVoiceBackend().then((backend) => {
      if (backend === 'none') {
        speechSupported = false;
        return;
      }
      speechSupported = true;
      voiceSession = createVoiceSession(backend, voiceCallbacks());
    });
  }

  onMount(() => {
    initVoiceSession();

    // React to backend override changes (skip initial emit)
    let firstEmit = true;
    const unsub = voiceBackendOverride.subscribe(() => {
      if (firstEmit) {
        firstEmit = false;
        return;
      }
      initVoiceSession();
    });

    return () => {
      unsub();
      voiceSession?.destroy();
    };
  });

  function toggleVoiceInput() {
    if (!voiceSession) return;

    if (isListening) {
      voiceSession.stop();
    } else {
      frequencyBuffer.fill(0);
      messageBeforeVoice = message;
      voiceSession.start();
    }
  }

  function handleSubmit() {
    if (!message.trim()) return;

    // Stop voice input if active
    if (isListening && voiceSession) {
      voiceSession.stop();
    }

    // During startup, queue message as a task instead of sending
    if (isInstanceStarting && effectiveInstanceId) {
      quickAddTask(effectiveInstanceId, message.trim());
      message = '';
      messageBeforeVoice = '';
      queueFlash = true;
      setTimeout(() => {
        queueFlash = false;
      }, 400);
      inputEl?.focus();
      return;
    }

    // If a task was staged, append a structural tag and send with task_id
    if ($stagedTask) {
      const tag = `\n[task:#${$stagedTask.id}]`;
      const taggedMessage = message + tag;
      sendMessage(taggedMessage, $stagedTask.id);
      commitStagedTask(taggedMessage);
    } else {
      sendMessage(message);
    }

    message = '';
    messageBeforeVoice = '';

    // Refocus input
    inputEl?.focus();
  }

  function handleAddToQueue() {
    if (!message.trim() || !effectiveInstanceId) return;

    if (isListening && voiceSession) {
      voiceSession.stop();
    }

    quickAddTask(effectiveInstanceId, message.trim());
    message = '';
    messageBeforeVoice = '';

    // Flash confirmation
    queueFlash = true;
    setTimeout(() => {
      queueFlash = false;
    }, 400);

    inputEl?.focus();
  }

  function hsSelect(index: number) {
    const result = hsResults[index];
    if (!result) return;
    const match = result.matches.find((m) => m.role === 'user') ?? result.matches[0];
    if (!match) return;
    message = match.snippet.replace(/<[^>]*>/g, '');
    historySearch = { mode: 'closed' };
    hsClear();
    requestAnimationFrame(() => {
      inputEl?.focus();
      if (inputEl) autoResize(inputEl);
    });
  }

  function hsClose() {
    if (historySearch.mode === 'open') {
      message = historySearch.savedMessage;
    }
    historySearch = { mode: 'closed' };
    hsClear();
    requestAnimationFrame(() => {
      inputEl?.focus();
      if (inputEl) autoResize(inputEl);
    });
  }

  function handleKeydown(e: KeyboardEvent) {
    // History search overlay — intercept keys before normal handling
    if (historySearch.mode === 'open') {
      if (e.key === 'Escape') {
        e.preventDefault();
        hsClose();
        return;
      }
      if (e.key === 'ArrowUp' && hsResults.length > 0) {
        e.preventDefault();
        hsSelectedIndex = Math.min(hsSelectedIndex + 1, hsResults.length - 1);
        return;
      }
      if (e.key === 'ArrowDown' && hsResults.length > 0) {
        e.preventDefault();
        hsSelectedIndex = Math.max(hsSelectedIndex - 1, 0);
        return;
      }
      if (e.key === 'r' && e.ctrlKey && !e.shiftKey && !e.metaKey && hsResults.length > 0) {
        e.preventDefault();
        hsSelectedIndex = Math.min(hsSelectedIndex + 1, hsResults.length - 1);
        return;
      }
      if (e.key === 'Enter' && !e.shiftKey) {
        e.preventDefault();
        if (hsResults.length > 0) {
          hsSelect(hsSelectedIndex);
        }
        return;
      }
      // All other keys: continue typing in the textarea, search triggers via $effect
      return;
    }

    // Open history search
    if (e.key === 'r' && e.ctrlKey && !e.shiftKey && !e.metaKey) {
      e.preventDefault();
      historySearch = { mode: 'open', savedMessage: message };
      // If there's already text, fire search immediately (skip debounce)
      const q = message.trim();
      if (q) {
        hsSearch(q);
      }
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey && (e.metaKey || e.ctrlKey)) {
      // Cmd/Ctrl+Shift handled below — but Cmd+Enter sends normally
    }
    if (e.key === 'Enter' && e.shiftKey && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      handleAddToQueue();
      return;
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  }

  // Auto-resize textarea - reactively tracks message content
  function autoResize(el: HTMLTextAreaElement) {
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, 200) + 'px';
  }

  $effect(() => {
    // Track message to re-run when content changes (including deletions)
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    message;
    if (inputEl) {
      autoResize(inputEl);
    }
  });

  // Persist draft to store on every change (survives instance switches & reloads).
  // Skip during history search — don't persist search queries as drafts.
  $effect(() => {
    if (historySearch.mode === 'open') return;
    const id = effectiveInstanceId;
    if (id) {
      setDraft(id, message);
    }
  });
</script>

<div class="input-container" class:disconnected={isDisconnected || isServerGone} class:reconnecting={isReconnecting}>
  {#if historySearch.mode === 'open'}
    <HistorySearchOverlay
      results={hsResults}
      loading={hsLoading}
      selectedIndex={hsSelectedIndex}
      hasQuery={message.trim().length > 0}
      onselect={hsSelect}
    />
  {/if}
  {#if $stagedTask}
    <div class="staged-banner">
      <svg class="staged-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path
          d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2"
        />
      </svg>
      <span class="staged-label">Task:</span>
      <span class="staged-title">{$stagedTask.title}</span>
      <button class="staged-dismiss" onclick={clearStagedTask} aria-label="Dismiss task" title="Dismiss (keep editing)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M6 18L18 6M6 6l12 12" />
        </svg>
      </button>
    </div>
  {/if}
  {#if showDraftBanner}
    <div class="voice-draft-banner">
      <span class="voice-draft-label">DRAFT</span>
      <span class="voice-draft-hint">stop to get corrected transcription</span>
    </div>
  {:else if showRecordingBanner}
    <div class="voice-draft-banner">
      <span class="voice-draft-label">RECORDING</span>
      <span class="voice-draft-hint">transcription on stop</span>
    </div>
  {:else if showCorrectingBanner}
    <div class="voice-draft-banner correcting">
      <svg class="spinner-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path
          d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48 8.48l2.83-2.83M2 12h4m12 0h4M4.93 4.93l2.83 2.83m8.48 8.48l2.83 2.83"
        />
      </svg>
      <span class="voice-correcting-label">transcribing&hellip;</span>
    </div>
  {/if}
  {#if showStartupBanner}
    <div class="status-banner startup">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="10" />
        <path d="M12 6v6l4 2" />
      </svg>
      <span>Claude is starting up — your message will be queued as a task</span>
    </div>
  {/if}
  {#if showBanner}
    <div
      class="status-banner"
      class:warning={isDisconnected || isServerGone}
      class:info={isReconnecting && !isDisconnected && !isServerGone}
    >
      {#if isServerGone}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18.364 5.636a9 9 0 11-12.728 0M12 9v4m0 4h.01" />
        </svg>
        <span>{$shutdownReason || 'Server is offline'}</span>
        <button class="retry-btn" onclick={() => reconnect()}>Retry Now</button>
      {:else if isDisconnected}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M18.364 5.636a9 9 0 11-12.728 0M12 9v4m0 4h.01" />
        </svg>
        <span>Disconnected</span>
      {:else if isReconnecting}
        <svg class="spinner-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path
            d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48 8.48l2.83-2.83M2 12h4m12 0h4M4.93 4.93l2.83 2.83m8.48 8.48l2.83 2.83"
          />
        </svg>
        <span>Reconnecting...</span>
      {/if}
      {#if $hasPendingInput}
        <span class="pending-badge">Message queued — will send when connected</span>
      {/if}
    </div>
  {/if}
  {#if isListening}
    <VoiceVisualizer data={frequencyBuffer} />
  {/if}
  <div class="input-row">
    <textarea
      bind:this={inputEl}
      bind:value={message}
      onkeydown={handleKeydown}
      oninput={() => inputEl && autoResize(inputEl)}
      placeholder={isServerGone
        ? 'Server is offline — message will send when it restarts...'
        : isDisconnected
          ? 'Type here — will send when reconnected...'
          : isInstanceStarting
            ? 'Type here — will queue as task while Claude starts...'
            : historySearch.mode === 'open'
              ? 'Search your message history...'
              : 'Message Claude...'}
      rows="1"
      role={historySearch.mode === 'open' ? 'combobox' : undefined}
      aria-expanded={historySearch.mode === 'open' ? hsResults.length > 0 : undefined}
      aria-controls={historySearch.mode === 'open' ? 'hs-listbox' : undefined}
      aria-activedescendant={historySearch.mode === 'open' && hsResults.length > 0
        ? `hs-result-${hsSelectedIndex}`
        : undefined}
      aria-autocomplete={historySearch.mode === 'open' ? 'list' : undefined}
      class:voice-draft={showDraftBanner}
      class:voice-correcting={showCorrectingBanner}
    ></textarea>
    {#if speechSupported}
      <div class="voice-wrapper">
        <button
          class="voice-btn"
          class:listening={isListening}
          class:transcribing={isTranscribing}
          onclick={toggleVoiceInput}
          disabled={isTranscribing}
          aria-label={isTranscribing ? 'Transcribing...' : isListening ? 'Stop voice input' : 'Start voice input'}
          title={isTranscribing ? 'Transcribing...' : isListening ? 'Stop listening' : 'Voice input'}
        >
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            {#if isTranscribing}
              <!-- Spinner while transcribing -->
              <path
                d="M12 2v4m0 12v4m-7.07-3.93l2.83-2.83m8.48 8.48l2.83-2.83M2 12h4m12 0h4M4.93 4.93l2.83 2.83m8.48 8.48l2.83 2.83"
              />
            {:else if isListening}
              <!-- Stop icon when listening -->
              <rect x="6" y="6" width="12" height="12" rx="1" />
            {:else}
              <!-- Microphone icon -->
              <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z" />
              <path d="M19 10v2a7 7 0 0 1-14 0v-2" />
              <line x1="12" y1="19" x2="12" y2="23" />
              <line x1="8" y1="23" x2="16" y2="23" />
            {/if}
          </svg>
        </button>
      </div>
    {/if}
    {#if isInstanceActive || isInstanceStarting}
      <button
        class="queue-btn"
        class:flash={queueFlash}
        onclick={handleAddToQueue}
        disabled={!message.trim()}
        aria-label="Add to queue (Cmd+Shift+Enter)"
        title="Add to queue (⌘⇧↵)"
      >
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M4 6h16M4 12h16M4 18h10" />
          <path d="M19 15l3 3-3 3" />
        </svg>
      </button>
    {/if}
    <button class="send-btn" onclick={handleSubmit} disabled={!message.trim()} aria-label="Send message">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z" />
      </svg>
    </button>
  </div>
</div>

<style>
  .input-container {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 0;
    padding: 0;
    background: linear-gradient(180deg, var(--surface-600) 0%, var(--surface-700) 100%);
    border-top: 1px solid var(--surface-border);
  }

  .input-container.disconnected {
    border-top-color: var(--status-red-border);
  }

  .input-container.reconnecting {
    border-top-color: var(--tint-selection);
  }

  .status-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .status-banner.warning {
    background: var(--status-red-tint);
    color: var(--status-red-text);
    border-bottom: 1px solid var(--status-red-border);
  }

  .status-banner.info {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
    border-bottom: 1px solid var(--tint-focus);
  }

  .status-banner.startup {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-400);
    border-bottom: 1px solid var(--tint-focus);
    animation: staged-slide-in 0.2s ease-out;
  }

  .status-banner.startup svg {
    animation: spin 2s linear infinite;
  }

  .status-banner svg {
    width: 14px;
    height: 14px;
    flex-shrink: 0;
  }

  .spinner-icon {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from {
      transform: rotate(0deg);
    }
    to {
      transform: rotate(360deg);
    }
  }

  .retry-btn {
    margin-left: auto;
    padding: 4px 10px;
    background: var(--tint-focus);
    border: 1px solid var(--status-red-border);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    font-family: inherit;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--status-red-text);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .retry-btn:hover {
    background: var(--status-red-tint);
    border-color: var(--status-red);
  }

  .pending-badge {
    margin-left: auto;
    padding: 4px 10px;
    background: var(--tint-focus);
    border: 1px solid var(--tint-selection);
    border-radius: 4px;
    font-size: 10px;
    font-weight: 600;
    color: var(--chrome-accent-400);
  }

  .input-row {
    display: flex;
    align-items: flex-end;
    gap: 10px;
    padding: 14px 16px;
  }

  textarea {
    flex: 1;
    padding: 12px 14px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-size: 13px;
    font-family: inherit;
    line-height: 1.5;
    resize: none;
    min-height: 44px;
    max-height: 200px;
    transition: all 0.15s ease;
  }

  textarea:focus {
    outline: none;
    border-color: var(--chrome-accent-600);
    box-shadow: var(--elevation-low);
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  .voice-wrapper {
    position: relative;
    flex-shrink: 0;
  }

  .voice-btn,
  .send-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .send-btn {
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .voice-btn:hover {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
  }

  .voice-btn.listening {
    background: linear-gradient(180deg, var(--status-red-strong) 0%, var(--status-red-tint) 100%);
    border-color: var(--status-red);
    color: var(--status-red-text);
    animation: pulse-glow 1.5s ease-in-out infinite;
  }

  .voice-btn.transcribing {
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border-color: var(--chrome-accent-600);
    color: var(--chrome-accent-400);
    cursor: wait;
  }

  .voice-btn.transcribing svg {
    animation: spin 1s linear infinite;
  }

  @keyframes pulse-glow {
    0%,
    100% {
      box-shadow: 0 0 8px var(--status-red-border);
    }
    50% {
      box-shadow: 0 0 16px var(--status-red-strong);
    }
  }

  .voice-btn svg,
  .send-btn svg {
    width: 18px;
    height: 18px;
  }

  .send-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-300);
    box-shadow: var(--elevation-high);
  }

  .send-btn:disabled {
    background: var(--surface-700);
    border-color: var(--surface-border);
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .queue-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    border: 1px solid var(--tint-focus);
    border-radius: 4px;
    color: var(--chrome-accent-500);
    cursor: pointer;
    transition: all 0.15s ease;
    flex-shrink: 0;
  }

  .queue-btn svg {
    width: 18px;
    height: 18px;
  }

  .queue-btn:hover:not(:disabled) {
    background: linear-gradient(180deg, var(--surface-400) 0%, var(--surface-500) 100%);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-300);
    box-shadow: var(--elevation-low);
  }

  .queue-btn:disabled {
    background: var(--surface-700);
    border-color: var(--surface-border);
    color: var(--text-muted);
    cursor: not-allowed;
  }

  .queue-btn.flash {
    animation: queue-flash 0.4s ease-out;
  }

  @keyframes queue-flash {
    0% {
      background: var(--chrome-accent-600);
      border-color: var(--chrome-accent-400);
    }
    100% {
      background: linear-gradient(180deg, var(--surface-500) 0%, var(--surface-600) 100%);
    }
  }

  /* Voice draft indicators */
  .voice-draft-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 16px;
    background: var(--tint-active-strong);
    border-bottom: 1px solid var(--chrome-accent-600);
    animation: staged-slide-in 0.2s ease-out;
  }

  .voice-draft-banner.correcting {
    background: var(--tint-active-strong);
  }

  .voice-draft-banner svg {
    width: 12px;
    height: 12px;
    flex-shrink: 0;
    color: var(--chrome-accent-400);
  }

  .voice-draft-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
  }

  .voice-draft-hint {
    font-size: 10px;
    font-style: italic;
    color: var(--text-muted);
    letter-spacing: 0.02em;
  }

  .voice-correcting-label {
    font-size: 10px;
    font-style: italic;
    font-weight: 500;
    letter-spacing: 0.04em;
    color: var(--chrome-accent-400);
  }

  textarea.voice-draft,
  textarea.voice-correcting {
    font-style: italic;
    color: var(--text-muted);
  }

  textarea.voice-draft {
    border-style: dashed;
    border-color: var(--chrome-accent-600);
  }

  /* Mobile responsive */
  @media (max-width: 639px) {
    .input-row {
      padding: 10px 12px;
      gap: 8px;
    }

    textarea {
      padding: 10px 12px;
      font-size: 16px; /* Prevents iOS zoom on focus */
    }

    .voice-btn,
    .send-btn,
    .queue-btn {
      width: 48px;
      height: 48px;
    }

    .status-banner {
      padding: 8px 12px;
      font-size: 10px;
      flex-wrap: wrap;
    }

    .pending-badge {
      margin-left: 0;
      margin-top: 6px;
      width: 100%;
      text-align: center;
    }
  }

  /* === Staged task banner === */
  .staged-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    background: var(--tint-active-strong);
    border-bottom: 1px solid var(--chrome-accent-600);
    animation: staged-slide-in 0.2s ease-out;
  }

  @keyframes staged-slide-in {
    from {
      opacity: 0;
      max-height: 0;
    }
    to {
      opacity: 1;
      max-height: 48px;
    }
  }

  .staged-icon {
    width: 14px;
    height: 14px;
    color: var(--chrome-accent-500);
    flex-shrink: 0;
  }

  .staged-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--chrome-accent-400);
    flex-shrink: 0;
  }

  .staged-title {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    flex: 1;
    min-width: 0;
  }

  .staged-dismiss {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    background: none;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-muted);
    cursor: pointer;
    flex-shrink: 0;
    transition: all 0.15s ease;
  }

  .staged-dismiss:hover {
    border-color: var(--surface-border-light);
    color: var(--text-secondary);
  }

  .staged-dismiss svg {
    width: 12px;
    height: 12px;
  }

  /* Analog overrides */
  :global([data-theme='analog']) .staged-banner {
    background-color: var(--tint-active-strong);
    background-image: var(--grain-fine);
    background-blend-mode: multiply;
    border-bottom-width: 2px;
  }

  :global([data-theme='analog']) .staged-label {
    font-family: 'Source Serif 4', Georgia, serif;
    font-style: italic;
    text-transform: none;
    letter-spacing: 0;
  }

  /* Safe area insets for mobile notches/home indicators */
  @supports (padding-bottom: env(safe-area-inset-bottom)) {
    .input-container {
      padding-bottom: env(safe-area-inset-bottom);
    }
  }
</style>
