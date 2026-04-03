<script lang="ts">
  import Sidebar from '$lib/components/Sidebar.svelte';
  import MainView from '$lib/components/MainView.svelte';
  import FileViewer from '$lib/components/FileViewer.svelte';
  import FileExplorer from '$lib/components/FileExplorer.svelte';
  import ChatPanel from '$lib/components/ChatPanel.svelte';
  import TaskPanel from '$lib/components/TaskPanel.svelte';
  import BootSequence from '$lib/components/BootSequence.svelte';
  import ChannelChange from '$lib/components/ChannelChange.svelte';
  import ServerShutdownModal from '$lib/components/ServerShutdownModal.svelte';
  import ToastStack from '$lib/components/ToastStack.svelte';
  import HistoryView from '$lib/components/HistoryView.svelte';
  import SettingsPanel from '$lib/components/settings/SettingsPanel.svelte';
  import CreateInstanceModal from '$lib/components/CreateInstanceModal.svelte';
  import { toggleExplorer, isExplorerOpen, closeExplorer } from '$lib/stores/files';
  import {
    isChatOpen,
    closeChat,
    toggleChat,
    composeOpen,
    closeCompose,
    selectionMode,
    exitSelectionMode
  } from '$lib/stores/chat';
  import { isTaskPanelOpen, closeTaskPanel, toggleTaskPanel } from '$lib/stores/tasks';
  import {
    isFileViewerOpen,
    closeFileViewer,
    currentFilePath,
    currentLineNumber,
    currentDiffContext,
    rootDirectory
  } from '$lib/stores/files';
  import { claudeState } from '$lib/stores/claude';
  import { connectionStatus } from '$lib/stores/websocket';
  import { currentProject } from '$lib/stores/projects';
  import { toggleTheme } from '$lib/stores/settings';
  import { selectInstance } from '$lib/stores/instances';
  import { fullscreenView, closeFullscreen, isFullscreenOpen } from '$lib/stores/fullscreen';
  import {
    layoutState,
    splitPane,
    splitFocusedPane,
    closePane,
    paneCount,
    moveFocus
  } from '$lib/stores/layout';

  let showBoot = $state(true);

  // Map Claude state to data attribute for ambient CSS
  const claudeStateAttr = $derived(
    $claudeState.type === 'Thinking'
      ? 'thinking'
      : $claudeState.type === 'Responding'
        ? 'responding'
        : $claudeState.type === 'ToolExecuting'
          ? 'tool_executing'
          : 'idle'
  );

  // Bind data-claude-state to document.body for global CSS ambient shifts
  $effect(() => {
    document.body.setAttribute('data-claude-state', claudeStateAttr);
  });

  // Bind connection status for signal-lost visual effects
  $effect(() => {
    document.body.setAttribute('data-connection', $connectionStatus);
  });

  function handleKeydown(e: KeyboardEvent) {
    // Cmd/Ctrl+Shift+L toggles theme
    if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key === 'L') {
      e.preventDefault();
      toggleTheme();
      return;
    }

    // Escape closes fullscreen → fileViewer → compose → selection → chat → tasks → explorer → sidebar (priority order)
    if (e.key === 'Escape') {
      if ($isFullscreenOpen && $fullscreenView !== 'new-project') {
        closeFullscreen();
      } else if ($isFileViewerOpen) {
        closeFileViewer();
      } else if ($composeOpen) {
        closeCompose();
      } else if ($selectionMode) {
        exitSelectionMode();
      } else if ($isChatOpen) {
        closeChat();
      } else if ($isTaskPanelOpen) {
        closeTaskPanel();
      } else if ($isExplorerOpen) {
        closeExplorer();
      }
    }
    // Ctrl+Arrow moves focus between panes
    if (e.ctrlKey && !e.metaKey && !e.shiftKey && !e.altKey) {
      if (e.key === 'ArrowLeft') {
        e.preventDefault();
        moveFocus('left');
        return;
      }
      if (e.key === 'ArrowRight') {
        e.preventDefault();
        moveFocus('right');
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        moveFocus('up');
        return;
      }
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        moveFocus('down');
        return;
      }
    }

    // Cmd/Ctrl+E toggles file explorer
    if ((e.metaKey || e.ctrlKey) && e.key === 'e') {
      e.preventDefault();
      toggleExplorer();
    }

    // Cmd/Ctrl+\ — split focused pane vertically
    if ((e.metaKey || e.ctrlKey) && e.key === '\\') {
      e.preventDefault();
      const focusedId = $layoutState.focusedPaneId;
      splitPane(focusedId, 'vertical');
      return;
    }
    // Cmd/Ctrl+W — close focused pane
    if ((e.metaKey || e.ctrlKey) && e.key === 'w') {
      if ($paneCount > 1) {
        e.preventDefault();
        closePane($layoutState.focusedPaneId);
        return;
      }
    }

    // --- Non-input shortcuts (disabled when typing or with modifiers) ---
    const tag = (document.activeElement?.tagName ?? '').toLowerCase();
    if (tag === 'input' || tag === 'textarea') return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;

    // / to focus message input
    if (e.key === '/') {
      e.preventDefault();
      const textarea = document.querySelector('textarea');
      textarea?.focus();
    }
    // f to toggle file explorer
    if (e.key === 'f') {
      e.preventDefault();
      toggleExplorer();
    }
    // c to toggle chat
    if (e.key === 'c') {
      e.preventDefault();
      toggleChat();
    }
    // q to toggle task panel
    if (e.key === 'q') {
      e.preventDefault();
      toggleTaskPanel();
    }
    // 1-9 to switch instances (indexes into current project's instances)
    const num = parseInt(e.key);
    if (num >= 1 && num <= 9) {
      const project = $currentProject;
      if (project && num <= project.instances.length) {
        e.preventDefault();
        selectInstance(project.instances[num - 1].id);
      }
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="app-container">
  <!-- Project rail sidebar — always visible -->
  <Sidebar />

  {#if $fullscreenView === 'history'}
    <HistoryView onclose={closeFullscreen} />
  {:else if $fullscreenView === 'settings'}
    <SettingsPanel onback={closeFullscreen} />
  {:else if $fullscreenView === 'new-project'}
    <CreateInstanceModal mode="project" onclose={closeFullscreen} oncreated={() => closeFullscreen()} />
  {:else}
    <MainView />

    <!-- Overlay drawer panels (always available; each has its own isVisible check) -->
    <FileExplorer
      oninset={() => {
        splitFocusedPane('file-explorer');
        closeExplorer();
      }}
    />
    <ChatPanel
      oninset={() => {
        splitFocusedPane('chat');
        closeChat();
      }}
    />
    <TaskPanel
      oninset={() => {
        splitFocusedPane('tasks');
        closeTaskPanel();
      }}
    />
    <FileViewer
      oninset={() => {
        const filePath = $currentFilePath;
        const lineNumber = $currentLineNumber ?? undefined;
        const diffContext = $currentDiffContext ?? undefined;
        splitPane($layoutState.focusedPaneId, 'vertical', {
          kind: 'file-viewer',
          filePath,
          lineNumber,
          workingDir: $rootDirectory,
          diffContext
        });
        closeFileViewer();
      }}
    />
  {/if}
</div>

<ChannelChange />
<ServerShutdownModal />
<ToastStack />

{#if showBoot}
  <BootSequence onComplete={() => (showBoot = false)} wsConnected={$connectionStatus === 'connected'} />
{/if}

<style>
  .app-container {
    display: flex;
    height: 100vh;
    height: 100dvh;
    width: 100vw;
    overflow: hidden;
    position: relative;
  }
</style>
