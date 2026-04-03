<script lang="ts">
  import { onMount } from 'svelte';
  import ServerFieldRow from './ServerFieldRow.svelte';
  import {
    serverConfigState,
    fetchServerConfig,
    updateLocalField,
    resetLocal,
    applyConfig,
    applyProfileDefaults,
    isDirty,
    isFieldDirty,
  } from '$lib/stores/server-config';
  import { api } from '$lib/utils/api';

  interface Props {
    onTabChange?: (tabId: string) => void;
  }

  let { onTabChange }: Props = $props();

  function focusOnMount(node: HTMLElement) {
    node.focus();
  }

  let portError = $state('');
  let scrollbackError = $state('');
  let authGateError = $state('');

  // Inline create-admin form state
  let showCreateAdmin = $state(false);
  let adminUsername = $state('');
  let adminPassword = $state('');
  let adminCreating = $state(false);
  let adminCreateError = $state('');
  let adminCreatedHint = $state(false);

  onMount(() => {
    fetchServerConfig();
  });

  function handleProfileChange(e: Event) {
    const value = (e.target as HTMLSelectElement).value;
    if (value === 'custom') {
      updateLocalField('profile', null);
    } else {
      const updated = applyProfileDefaults($serverConfigState.local, value);
      updateLocalField('host', updated.host);
      updateLocalField('auth_enabled', updated.auth_enabled);
      updateLocalField('https', updated.https);

      // Profile presets can flip auth on — run the same admin check
      if (updated.auth_enabled && !$serverConfigState.server?.auth_enabled) {
        checkAdminExists();
      } else {
        authGateError = '';
        showCreateAdmin = false;
        adminCreatedHint = false;
      }
    }
  }

  function handlePortChange(e: Event) {
    const raw = (e.target as HTMLInputElement).value;
    const num = parseInt(raw, 10);
    if (isNaN(num) || num < 0 || num > 65535) {
      portError = 'Port must be 0-65535';
      return;
    }
    portError = '';
    updateLocalField('port', num);
  }

  function handleScrollbackChange(e: Event) {
    const raw = (e.target as HTMLInputElement).value;
    const num = parseInt(raw, 10);
    if (isNaN(num) || num < 100 || num > 100000) {
      scrollbackError = '100-100,000';
      return;
    }
    scrollbackError = '';
    updateLocalField('scrollback_lines', num);
  }

  function handleHostChange(e: Event) {
    updateLocalField('host', (e.target as HTMLInputElement).value);
  }

  async function checkAdminExists() {
    try {
      const resp = await api('/api/admin/users');
      if (resp.ok) {
        const users: Array<{ is_admin: boolean; is_disabled: boolean }> = await resp.json();
        const hasAdmin = users.some((u) => u.is_admin && !u.is_disabled);
        if (!hasAdmin) {
          authGateError = 'No admin accounts exist \u2014 you\u2019ll be locked out.';
          showCreateAdmin = true;
        } else {
          authGateError = '';
          showCreateAdmin = false;
        }
      }
    } catch {
      // If we can't check, let the backend guard catch it
    }
  }

  async function handleAuthToggle() {
    const turningOn = !$serverConfigState.local.auth_enabled;
    updateLocalField('auth_enabled', turningOn);

    if (turningOn && !$serverConfigState.server?.auth_enabled) {
      await checkAdminExists();
    } else {
      authGateError = '';
      showCreateAdmin = false;
      adminCreatedHint = false;
    }
  }

  async function handleCreateAdmin(e: Event) {
    e.preventDefault();
    adminCreateError = '';
    adminCreating = true;

    try {
      const resp = await api('/api/admin/users', {
        method: 'POST',
        body: JSON.stringify({
          username: adminUsername.trim(),
          password: adminPassword,
          is_admin: true,
        }),
      });

      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: 'Failed to create admin' }));
        adminCreateError = data.error || 'Failed to create admin';
      } else {
        // Success — clear the gate, show the hint
        authGateError = '';
        showCreateAdmin = false;
        adminCreatedHint = true;
        adminUsername = '';
        adminPassword = '';
      }
    } catch {
      adminCreateError = 'Network error';
    }
    adminCreating = false;
  }

  function handleHttpsToggle() {
    updateLocalField('https', !$serverConfigState.local.https);
  }

  async function handleApply() {
    adminCreatedHint = false;
    await applyConfig(false);
    recoverAuthGateFromApplyError();
  }

  async function handleSaveApply() {
    adminCreatedHint = false;
    await applyConfig(true);
    recoverAuthGateFromApplyError();
  }

  /** If the backend rejected auth-enable, re-show the inline create-admin form. */
  function recoverAuthGateFromApplyError() {
    const error = $serverConfigState.error;
    if (error && error.toLowerCase().includes('no admin')) {
      authGateError = 'No admin accounts exist \u2014 you\u2019ll be locked out.';
      showCreateAdmin = true;
    }
  }

  function handleReset() {
    portError = '';
    scrollbackError = '';
    authGateError = '';
    showCreateAdmin = false;
    adminCreatedHint = false;
    adminCreateError = '';
    resetLocal();
  }

</script>

{#if $serverConfigState.loading}
  <div class="loading">Loading server configuration...</div>
{:else if $serverConfigState.error && !$serverConfigState.server}
  <div class="error-state">
    <p class="error-text">{$serverConfigState.error}</p>
    <button class="action-btn" onclick={() => fetchServerConfig()}>Retry</button>
  </div>
{:else}
  <!-- Profile -->
  <section class="settings-section">
    <h2 class="section-header">PROFILE</h2>
    <div class="profile-row">
      <div class="profile-info">
        <span class="profile-label">Deployment Mode</span>
        <span class="profile-desc">Presets for host, auth, and HTTPS</span>
      </div>
      <select
        class="profile-select"
        value={$serverConfigState.local.profile ?? 'custom'}
        onchange={handleProfileChange}
      >
        <option value="local">Local</option>
        <option value="tunnel">Tunnel</option>
        <option value="server">Server</option>
        <option value="custom">Custom</option>
      </select>
    </div>
  </section>

  <!-- Config Fields -->
  <section class="settings-section">
    <h2 class="section-header">CONFIGURATION</h2>

    <ServerFieldRow
      label="Host"
      description="Bind address"
      dirty={isFieldDirty($serverConfigState, 'host')}
      overridden={$serverConfigState.overrides.host}
    >
      <input
        type="text"
        class="field-input"
        value={$serverConfigState.local.host}
        onchange={handleHostChange}
      />
    </ServerFieldRow>

    <ServerFieldRow
      label="Port"
      description={portError || 'Listening port'}
      dirty={isFieldDirty($serverConfigState, 'port')}
      overridden={$serverConfigState.overrides.port}
    >
      <input
        type="number"
        class="field-input port-input"
        class:invalid={!!portError}
        value={$serverConfigState.local.port}
        min="0"
        max="65535"
        onchange={handlePortChange}
      />
    </ServerFieldRow>

    <ServerFieldRow
      label="Auth"
      description="Require authentication"
      dirty={isFieldDirty($serverConfigState, 'auth_enabled')}
      overridden={$serverConfigState.overrides.auth_enabled}
    >
      <button
        class="indicator-btn"
        class:on={$serverConfigState.local.auth_enabled}
        onclick={handleAuthToggle}
      >
        <span class="indicator-dot"></span>
        <span class="indicator-label">{$serverConfigState.local.auth_enabled ? 'ON' : 'OFF'}</span>
      </button>
    </ServerFieldRow>

    {#if authGateError}
      <div class="auth-gate-notice">
        <div class="auth-gate-text" class:with-form={showCreateAdmin}>{authGateError}</div>
        {#if showCreateAdmin}
          <form class="inline-admin-form" onsubmit={handleCreateAdmin}>
            {#if adminCreateError}
              <div class="inline-error">{adminCreateError}</div>
            {/if}
            <div class="inline-fields">
              <input
                type="text"
                class="inline-input"
                bind:value={adminUsername}
                placeholder="Username"
                disabled={adminCreating}
                required
                minlength={2}
                maxlength={64}
                use:focusOnMount
              />
              <input
                type="password"
                class="inline-input"
                bind:value={adminPassword}
                placeholder="Password (8+ chars)"
                disabled={adminCreating}
                required
                minlength={8}
              />
              <button class="inline-create-btn" type="submit" disabled={adminCreating}>
                {adminCreating ? '...' : 'Create Admin'}
              </button>
            </div>
          </form>
        {/if}
      </div>
    {/if}

    {#if adminCreatedHint}
      <div class="admin-created-hint">
        Admin account created.
        {#if onTabChange}
          <button class="tab-link" onclick={() => onTabChange?.('admin')}>
            Manage users in Admin tab
          </button>
        {/if}
      </div>
    {/if}

    <ServerFieldRow
      label="HTTPS"
      description="TLS encryption"
      dirty={isFieldDirty($serverConfigState, 'https')}
      overridden={$serverConfigState.overrides.https}
    >
      <button
        class="indicator-btn"
        class:on={$serverConfigState.local.https}
        onclick={handleHttpsToggle}
      >
        <span class="indicator-dot"></span>
        <span class="indicator-label">{$serverConfigState.local.https ? 'ON' : 'OFF'}</span>
      </button>
    </ServerFieldRow>

    <ServerFieldRow
      label="Scrollback"
      description={scrollbackError || 'Terminal buffer lines'}
      dirty={isFieldDirty($serverConfigState, 'scrollback_lines')}
      overridden={$serverConfigState.overrides.scrollback_lines}
    >
      <input
        type="number"
        class="field-input scrollback-input"
        class:invalid={!!scrollbackError}
        value={$serverConfigState.local.scrollback_lines}
        min="100"
        max="100000"
        onchange={handleScrollbackChange}
      />
    </ServerFieldRow>
  </section>

  <!-- Action Bar -->
  <section class="action-bar">
    {#if isDirty($serverConfigState)}
      <button class="reset-btn" onclick={handleReset}>Reset</button>
    {/if}
    <button
      class="apply-btn"
      disabled={!isDirty($serverConfigState) || $serverConfigState.applying || !!portError || !!scrollbackError || !!authGateError}
      onclick={handleApply}
    >
      {$serverConfigState.applying ? 'Applying...' : 'APPLY'}
    </button>
    <button
      class="save-apply-btn"
      disabled={!isDirty($serverConfigState) || $serverConfigState.applying || !!portError || !!scrollbackError || !!authGateError}
      onclick={handleSaveApply}
    >
      {$serverConfigState.applying ? 'Saving...' : 'SAVE + APPLY'}
    </button>
  </section>

  {#if $serverConfigState.statusMessage}
    <div class="status-feedback success-msg">{$serverConfigState.statusMessage}</div>
  {/if}
  {#if $serverConfigState.error && $serverConfigState.server}
    <div class="status-feedback error-msg">{$serverConfigState.error}</div>
  {/if}
{/if}

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

  .loading {
    padding: 24px 0;
    font-size: 11px;
    color: var(--text-muted);
    letter-spacing: 0.03em;
  }

  .error-state {
    padding: 24px 0;
  }

  .error-text {
    font-size: 11px;
    color: var(--status-red);
    margin: 0 0 12px 0;
  }

  /* Profile */
  .profile-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 8px 0;
    gap: 16px;
  }

  .profile-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .profile-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.03em;
  }

  .profile-desc {
    font-size: 10px;
    color: var(--text-muted);
    letter-spacing: 0.02em;
  }

  .profile-select {
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

  .profile-select:hover {
    border-color: var(--chrome-accent-600);
  }

  .profile-select:focus {
    border-color: var(--chrome-accent-500);
  }

  .profile-select option {
    background: var(--surface-600);
    color: var(--text-primary);
  }

  .auth-gate-notice {
    padding: 8px 10px;
    margin: 4px 0 8px 0;
    border-radius: 4px;
    font-size: 10px;
    letter-spacing: 0.02em;
    background: var(--tint-active);
    border: 1px solid var(--tint-active);
    color: var(--chrome-accent-400);
    line-height: 1.4;
  }

  .auth-gate-text {
    margin-bottom: 0;
  }

  .auth-gate-text.with-form {
    margin-bottom: 8px;
  }

  .inline-admin-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .inline-error {
    font-size: 10px;
    color: var(--status-red);
    letter-spacing: 0.02em;
  }

  .inline-fields {
    display: flex;
    gap: 6px;
    align-items: center;
    flex-wrap: wrap;
  }

  .inline-input {
    font-size: 10px;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 3px;
    padding: 4px 6px;
    width: 120px;
    outline: none;
  }

  .inline-input:focus {
    border-color: var(--chrome-accent-500);
    box-shadow: var(--recess-border);
    color: var(--text-primary);
  }

  .inline-create-btn {
    padding: 4px 10px;
    background: var(--chrome-accent-600);
    border: none;
    border-radius: 3px;
    color: var(--surface-900);
    font-family: inherit;
    font-weight: 700;
    font-size: 10px;
    letter-spacing: 0.03em;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.15s ease;
  }

  .inline-create-btn:hover:not(:disabled) {
    background: var(--chrome-accent-500);
  }

  .inline-create-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .admin-created-hint {
    padding: 6px 10px;
    margin: 4px 0 8px 0;
    border-radius: 4px;
    font-size: 10px;
    letter-spacing: 0.02em;
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-tint);
    color: var(--status-green-text);
    line-height: 1.4;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .tab-link {
    background: none;
    border: none;
    color: var(--chrome-accent-400);
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.02em;
    cursor: pointer;
    padding: 0;
    text-decoration: underline;
    text-underline-offset: 2px;
  }

  .tab-link:hover {
    color: var(--chrome-accent-300);
  }

  /* Field inputs */
  .field-input {
    font-size: 11px;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    padding: 4px 8px;
    width: 120px;
    outline: none;
  }

  .field-input:hover {
    border-color: var(--chrome-accent-600);
  }

  .field-input:focus {
    border-color: var(--chrome-accent-500);
    box-shadow: var(--recess-border);
    color: var(--text-primary);
  }

  .field-input.invalid {
    border-color: var(--status-red);
  }

  .field-input.port-input {
    width: 72px;
  }

  .field-input.scrollback-input {
    width: 80px;
  }

  /* Indicator toggle */
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

  /* Action bar */
  .action-bar {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 12px 0;
    border-top: 1px solid var(--surface-border);
    margin-top: 8px;
  }

  .reset-btn {
    padding: 6px 12px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    font-family: inherit;
    font-weight: 600;
    font-size: 10px;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .reset-btn:hover {
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }

  .apply-btn {
    padding: 6px 12px;
    background: var(--surface-700);
    border: 1px solid var(--chrome-accent-600);
    border-radius: 4px;
    color: var(--chrome-accent-400);
    font-family: inherit;
    font-weight: 700;
    font-size: 10px;
    letter-spacing: 0.05em;
    cursor: pointer;
    box-shadow: var(--elevation-low);
    transition:
      all 0.15s ease,
      box-shadow 0.15s ease;
    margin-left: auto;
  }

  .apply-btn:hover:not(:disabled) {
    background: var(--surface-600);
    box-shadow: var(--elevation-high);
  }

  .apply-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .save-apply-btn {
    padding: 6px 12px;
    background: var(--chrome-accent-600);
    border: none;
    border-radius: 4px;
    color: var(--surface-900);
    font-family: inherit;
    font-weight: 700;
    font-size: 10px;
    letter-spacing: 0.05em;
    cursor: pointer;
    box-shadow: var(--depth-up);
    transition:
      background 0.15s ease,
      box-shadow 0.15s ease;
  }

  .save-apply-btn:hover:not(:disabled) {
    background: var(--chrome-accent-500);
    box-shadow: var(--elevation-high);
  }

  .save-apply-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .action-btn {
    padding: 6px 12px;
    background: var(--chrome-accent-600);
    border: none;
    border-radius: 4px;
    color: var(--surface-900);
    font-family: inherit;
    font-weight: 600;
    font-size: 11px;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: background 0.15s ease;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--chrome-accent-500);
  }

  /* Status feedback */
  .status-feedback {
    padding: 6px 10px;
    border-radius: 4px;
    font-size: 11px;
    letter-spacing: 0.02em;
    margin-top: 8px;
  }

  .success-msg {
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-tint);
    color: var(--status-green-text);
  }

  .error-msg {
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-tint);
    color: var(--status-red);
  }
</style>
