<script lang="ts">
  import {
    userSettings,
    updateSetting,
    setTheme,
    THEME_OPTIONS,
    DEFAULT_SETTINGS,
    type UserSettings,
    type ThemeId,
  } from '$lib/stores/settings';
  import { requestNotificationPermission } from '$lib/stores/inbox';

  let notificationDenied = $state(
    typeof window !== 'undefined' && 'Notification' in window && Notification.permission === 'denied'
  );

  function handleThemeChange(e: Event) {
    const value = (e.target as HTMLSelectElement).value as ThemeId;
    setTheme(value);
  }

  function handleDiffEngineChange(e: Event) {
    const value = (e.target as HTMLSelectElement).value as UserSettings['diffEngine'];
    updateSetting('diffEngine', value);
  }

  function handleDefaultCommandChange(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateSetting('defaultCommand', value);
  }

  function handleShellCommandChange(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateSetting('shellCommand', value);
  }

  function handleFontSizeChange(e: Event) {
    const value = Number((e.target as HTMLInputElement).value);
    updateSetting('terminalFontSize', value);
  }

  function handleFontFamilyChange(e: Event) {
    const value = (e.target as HTMLInputElement).value;
    updateSetting('terminalFontFamily', value);
  }

  async function handleNotificationsToggle() {
    const enabling = !$userSettings.showNotifications;
    if (enabling) {
      const result = await requestNotificationPermission();
      if (result === 'denied') {
        notificationDenied = true;
        return;
      }
    }
    updateSetting('showNotifications', enabling);
  }
</script>

<!-- Appearance -->
<section class="settings-section">
  <h2 class="section-header">APPEARANCE</h2>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="theme-select">Theme</label>
      <span class="setting-desc">Visual style for the interface</span>
    </div>
    <select
      id="theme-select"
      class="setting-select"
      value={$userSettings.theme}
      onchange={handleThemeChange}
    >
      {#each THEME_OPTIONS as opt (opt.id)}
        <option value={opt.id}>{opt.label}</option>
      {/each}
    </select>
  </div>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="font-size">Terminal Font Size</label>
      <span class="setting-desc">{$userSettings.terminalFontSize}px</span>
    </div>
    <input
      id="font-size"
      type="range"
      min="12"
      max="24"
      step="1"
      value={$userSettings.terminalFontSize}
      oninput={handleFontSizeChange}
      class="range-input"
    />
  </div>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="font-family">Terminal Font</label>
      <span class="setting-desc">CSS font-family stack</span>
    </div>
    <input
      id="font-family"
      type="text"
      class="setting-input font-family-input"
      value={$userSettings.terminalFontFamily}
      onchange={handleFontFamilyChange}
      placeholder={DEFAULT_SETTINGS.terminalFontFamily}
    />
  </div>
</section>

<!-- Editor -->
<section class="settings-section">
  <h2 class="section-header">EDITOR</h2>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="diff-engine">Diff Engine</label>
      <span class="setting-desc">Algorithm for displaying code diffs</span>
    </div>
    <select
      id="diff-engine"
      class="setting-select"
      value={$userSettings.diffEngine}
      onchange={handleDiffEngineChange}
    >
      <option value="standard">Standard</option>
      <option value="patience">Patience</option>
      <option value="structural">Structural</option>
    </select>
  </div>
</section>

<!-- Terminal -->
<section class="settings-section">
  <h2 class="section-header">TERMINAL</h2>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="default-command">Default Command</label>
      <span class="setting-desc">Command used for new Claude instances</span>
    </div>
    <input
      id="default-command"
      type="text"
      class="setting-input"
      value={$userSettings.defaultCommand}
      onchange={handleDefaultCommandChange}
      placeholder={DEFAULT_SETTINGS.defaultCommand}
    />
  </div>

  <div class="setting-row">
    <div class="setting-info">
      <label class="setting-label" for="shell-command">Shell Command</label>
      <span class="setting-desc">Command used for new terminal panes</span>
    </div>
    <input
      id="shell-command"
      type="text"
      class="setting-input"
      value={$userSettings.shellCommand}
      onchange={handleShellCommandChange}
      placeholder={DEFAULT_SETTINGS.shellCommand}
    />
  </div>

  <div class="setting-row">
    <div class="setting-info">
      <span class="setting-label">Notifications</span>
      <span class="setting-desc"
        >{notificationDenied
          ? 'Blocked by browser — reset in site settings'
          : 'Browser alerts when instances are ready'}</span
      >
    </div>
    <button
      class="indicator-btn"
      class:on={$userSettings.showNotifications && !notificationDenied}
      disabled={notificationDenied}
      onclick={handleNotificationsToggle}
      aria-label="Toggle notifications"
    >
      <span class="indicator-dot"></span>
      <span class="indicator-label"
        >{notificationDenied ? 'BLOCKED' : $userSettings.showNotifications ? 'ON' : 'OFF'}</span
      >
    </button>
  </div>
</section>

<style>
  .settings-section {
    margin-bottom: 24px;
  }

  .section-header {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.1em;
    color: var(--chrome-accent-500);
    text-shadow: var(--emphasis);
    margin: 0 0 12px 0;
    padding-bottom: 6px;
    border-bottom: 1px solid var(--surface-border);
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 0;
    gap: 16px;
  }

  .setting-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .setting-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.03em;
  }

  .setting-desc {
    font-size: 10px;
    color: var(--text-muted);
    letter-spacing: 0.02em;
  }

  .range-input {
    width: 100px;
    height: 4px;
    appearance: none;
    -webkit-appearance: none;
    background: var(--surface-600);
    border-radius: 2px;
    outline: none;
    cursor: pointer;
  }

  .range-input::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--chrome-accent-500);
    box-shadow: 0 0 4px var(--chrome-accent-500);
    border: none;
    cursor: pointer;
  }

  .range-input::-moz-range-thumb {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--chrome-accent-500);
    box-shadow: 0 0 4px var(--chrome-accent-500);
    border: none;
    cursor: pointer;
  }

  .setting-select {
    font-size: 11px;
    font-weight: 600;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    padding: 4px 8px;
    cursor: pointer;
    outline: none;
  }

  .setting-select:hover {
    border-color: var(--chrome-accent-600);
  }

  .setting-select:focus {
    border-color: var(--chrome-accent-500);
    box-shadow: var(--recess-border);
  }

  .setting-select option {
    background: var(--surface-600);
    color: var(--text-primary);
  }

  .setting-input {
    font-size: 11px;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    padding: 4px 8px;
    width: 140px;
    outline: none;
  }

  .setting-input:hover {
    border-color: var(--chrome-accent-600);
  }

  .setting-input:focus {
    border-color: var(--chrome-accent-500);
    box-shadow: var(--recess-border);
    color: var(--text-primary);
  }

  .setting-input::placeholder {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .setting-input.font-family-input {
    width: 220px;
    font-size: 10px;
  }

  .indicator-btn {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    padding: 4px 8px;
    cursor: pointer;
    font-family: inherit;
    transition: border-color 0.15s ease;
  }

  .indicator-btn:hover {
    border-color: var(--chrome-accent-600);
  }

  .indicator-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
    opacity: 0.3;
    transition: all 0.15s ease;
  }

  .indicator-btn.on .indicator-dot {
    background: var(--chrome-accent-500);
    box-shadow: 0 0 4px var(--chrome-accent-500);
    opacity: 1;
  }

  .indicator-label {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.08em;
    color: var(--text-muted);
  }

  .indicator-btn.on .indicator-label {
    color: var(--chrome-accent-400);
  }
</style>
