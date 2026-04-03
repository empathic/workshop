<script lang="ts">
  import { currentUser, changePassword, logout } from '$lib/stores/auth';
  import { base } from '$app/paths';

  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let pwError = $state('');
  let pwSuccess = $state('');
  let pwLoading = $state(false);

  async function handleChangePassword(e: Event) {
    e.preventDefault();
    pwError = '';
    pwSuccess = '';

    if (newPassword.length < 8) {
      pwError = 'New password must be at least 8 characters';
      return;
    }

    if (newPassword !== confirmPassword) {
      pwError = 'New passwords do not match';
      return;
    }

    pwLoading = true;
    const result = await changePassword(currentPassword, newPassword);
    pwLoading = false;

    if (result.ok) {
      pwSuccess = 'Password changed successfully';
      currentPassword = '';
      newPassword = '';
      confirmPassword = '';
    } else {
      pwError = result.error ?? 'Failed to change password';
    }
  }

  async function handleLogout() {
    await logout();
    window.location.href = `${base}/login`;
  }
</script>

{#if $currentUser}
  <!-- Account Info -->
  <section class="settings-section">
    <h2 class="section-header">ACCOUNT</h2>

    <div class="about-row">
      <span class="about-key">Username</span>
      <span class="about-value">{$currentUser.username}</span>
    </div>
    <div class="about-row">
      <span class="about-key">Display Name</span>
      <span class="about-value">{$currentUser.display_name}</span>
    </div>
    {#if $currentUser.is_admin}
      <div class="about-row">
        <span class="about-key">Role</span>
        <span class="about-value admin-badge">ADMIN</span>
      </div>
    {/if}
  </section>

  <!-- Change Password -->
  <section class="settings-section">
    <h2 class="section-header">CHANGE PASSWORD</h2>

    {#if pwError}
      <div class="feedback-msg error-msg">{pwError}</div>
    {/if}
    {#if pwSuccess}
      <div class="feedback-msg success-msg">{pwSuccess}</div>
    {/if}

    <form class="pw-form" onsubmit={handleChangePassword}>
      <div class="form-field">
        <label class="setting-label" for="pw-current">Current Password</label>
        <input
          id="pw-current"
          type="password"
          class="setting-input full-width"
          bind:value={currentPassword}
          required
          autocomplete="current-password"
          disabled={pwLoading}
        />
      </div>
      <div class="form-field">
        <label class="setting-label" for="pw-new">New Password</label>
        <input
          id="pw-new"
          type="password"
          class="setting-input full-width"
          bind:value={newPassword}
          required
          minlength="8"
          autocomplete="new-password"
          disabled={pwLoading}
        />
      </div>
      <div class="form-field">
        <label class="setting-label" for="pw-confirm">Confirm New Password</label>
        <input
          id="pw-confirm"
          type="password"
          class="setting-input full-width"
          bind:value={confirmPassword}
          required
          minlength="8"
          autocomplete="new-password"
          disabled={pwLoading}
        />
      </div>
      <button class="action-btn" type="submit" disabled={pwLoading}>
        {pwLoading ? 'Changing...' : 'Change Password'}
      </button>
    </form>
  </section>

  <!-- Sign Out -->
  <section class="settings-section">
    <button class="signout-btn" onclick={handleLogout}>Sign Out</button>
  </section>
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

  .about-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 0;
  }

  .about-key {
    font-size: 11px;
    color: var(--text-muted);
    letter-spacing: 0.03em;
  }

  .about-value {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.03em;
  }

  .about-value.admin-badge {
    color: var(--chrome-accent-400);
    font-size: 10px;
    letter-spacing: 0.08em;
  }

  .form-field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 10px;
  }

  .setting-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    letter-spacing: 0.03em;
  }

  .setting-input {
    font-size: 11px;
    font-family: inherit;
    color: var(--text-secondary);
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    padding: 4px 8px;
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

  .setting-input.full-width {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 8px;
  }

  .pw-form {
    display: flex;
    flex-direction: column;
  }

  .feedback-msg {
    padding: 6px 10px;
    margin-bottom: 10px;
    border-radius: 4px;
    font-size: 11px;
    letter-spacing: 0.02em;
  }

  .error-msg {
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-tint);
    color: var(--status-red);
  }

  .success-msg {
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-tint);
    color: var(--status-green-text);
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
    align-self: flex-start;
    margin-top: 4px;
    box-shadow: var(--depth-up);
    transition:
      background 0.15s ease,
      box-shadow 0.15s ease;
  }

  .action-btn:hover:not(:disabled) {
    background: var(--chrome-accent-500);
    box-shadow: var(--elevation-high);
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .signout-btn {
    width: 100%;
    padding: 8px 12px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    font-family: inherit;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.05em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .signout-btn:hover {
    border-color: var(--status-red);
    color: var(--status-red);
  }
</style>
