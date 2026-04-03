<script lang="ts">
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { currentUser, changePassword, logout, csrfToken } from '$lib/stores/auth';
  import { api } from '$lib/utils/api';
  import { onMount } from 'svelte';

  let currentPassword = $state('');
  let newPassword = $state('');
  let confirmPassword = $state('');
  let error = $state('');
  let success = $state('');
  let loading = $state(false);

  // Server invite state
  interface InviteAcceptor {
    user_id: string;
    username: string;
    display_name: string;
    created_at: number;
  }

  interface ServerInvite {
    token: string;
    created_by: string;
    label: string | null;
    max_uses: number | null;
    use_count: number;
    expires_at: number | null;
    revoked: boolean;
    created_at: number;
  }

  interface ServerInviteWithAcceptors {
    invite: ServerInvite;
    acceptors: InviteAcceptor[];
  }

  let invites = $state<ServerInviteWithAcceptors[]>([]);
  let inviteLabel = $state('');
  let inviteMaxUses = $state('');
  let inviteExpiryHours = $state('');
  let inviteLoading = $state(false);
  let inviteError = $state('');
  let copiedToken = $state<string | null>(null);

  async function loadInvites() {
    try {
      const resp = await api('/api/admin/invites');
      if (resp.ok) {
        invites = await resp.json();
      }
    } catch {
      // ignore
    }
  }

  onMount(() => {
    const unsub = currentUser.subscribe((user) => {
      if (user?.is_admin) {
        loadInvites();
      }
    });
    return unsub;
  });

  async function handleCreateInvite(e: Event) {
    e.preventDefault();
    inviteError = '';
    inviteLoading = true;

    try {
      const body: Record<string, unknown> = {};
      if (inviteLabel.trim()) body.label = inviteLabel.trim();
      if (inviteMaxUses) body.max_uses = parseInt(inviteMaxUses);
      if (inviteExpiryHours) body.expires_in_hours = parseInt(inviteExpiryHours);

      const resp = await api('/api/admin/invites', {
        method: 'POST',
        body: JSON.stringify(body)
      });

      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: 'Failed to create invite' }));
        inviteError = data.error || 'Failed to create invite';
      } else {
        inviteLabel = '';
        inviteMaxUses = '';
        inviteExpiryHours = '';
        await loadInvites();
      }
    } catch {
      inviteError = 'Network error';
    }
    inviteLoading = false;
  }

  async function revokeInvite(token: string) {
    try {
      await api(`/api/admin/invites/${encodeURIComponent(token)}`, {
        method: 'DELETE'
      });
      await loadInvites();
    } catch {
      // ignore
    }
  }

  function copyInviteLink(token: string) {
    const url = `${window.location.origin}${base}/register?invite=${token}`;
    navigator.clipboard.writeText(url);
    copiedToken = token;
    setTimeout(() => {
      if (copiedToken === token) copiedToken = null;
    }, 2000);
  }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    });
  }

  function isExpired(invite: ServerInvite): boolean {
    return invite.expires_at !== null && Date.now() / 1000 > invite.expires_at;
  }

  function isUsedUp(invite: ServerInvite): boolean {
    return invite.max_uses !== null && invite.use_count >= invite.max_uses;
  }

  async function handleChangePassword(e: Event) {
    e.preventDefault();
    error = '';
    success = '';

    if (newPassword.length < 8) {
      error = 'New password must be at least 8 characters';
      return;
    }

    if (newPassword !== confirmPassword) {
      error = 'New passwords do not match';
      return;
    }

    loading = true;
    const result = await changePassword(currentPassword, newPassword);
    loading = false;

    if (result.ok) {
      success = 'Password changed successfully';
      currentPassword = '';
      newPassword = '';
      confirmPassword = '';
    } else {
      error = result.error ?? 'Failed to change password';
    }
  }

  async function handleLogout() {
    await logout();
    goto(`${base}/login`);
  }
</script>

<div class="auth-page">
  <div class="auth-card">
    <a class="back-link" href="{base}/">&larr; Dashboard</a>
    <h1>Account</h1>

    {#if $currentUser}
      <div class="user-info">
        <span class="label">Username</span>
        <span class="value">{$currentUser.username}</span>
      </div>
      <div class="user-info">
        <span class="label">Display Name</span>
        <span class="value">{$currentUser.display_name}</span>
      </div>
      {#if $currentUser.is_admin}
        <div class="admin-badge">Admin</div>
      {/if}

      <hr />

      <h2>Change Password</h2>

      {#if error}
        <div class="error">{error}</div>
      {/if}
      {#if success}
        <div class="success">{success}</div>
      {/if}

      <form onsubmit={handleChangePassword}>
        <label>
          <span>Current Password</span>
          <input
            type="password"
            bind:value={currentPassword}
            required
            autocomplete="current-password"
            disabled={loading}
          />
        </label>

        <label>
          <span>New Password</span>
          <input
            type="password"
            bind:value={newPassword}
            required
            minlength="8"
            autocomplete="new-password"
            disabled={loading}
          />
        </label>

        <label>
          <span>Confirm New Password</span>
          <input
            type="password"
            bind:value={confirmPassword}
            required
            minlength="8"
            autocomplete="new-password"
            disabled={loading}
          />
        </label>

        <button type="submit" disabled={loading}>
          {loading ? 'Changing...' : 'Change Password'}
        </button>
      </form>

      {#if $currentUser.is_admin}
        <hr />

        <h2>Server Invitations</h2>
        <p class="section-desc">Create invite links so new users can register.</p>

        {#if inviteError}
          <div class="error">{inviteError}</div>
        {/if}

        <form class="invite-form" onsubmit={handleCreateInvite}>
          <label>
            <span>Label (optional)</span>
            <input type="text" bind:value={inviteLabel} placeholder="e.g. For Alice" disabled={inviteLoading} />
          </label>
          <div class="invite-row">
            <label class="compact">
              <span>Max uses</span>
              <input
                type="number"
                bind:value={inviteMaxUses}
                placeholder="unlimited"
                min="1"
                disabled={inviteLoading}
              />
            </label>
            <label class="compact">
              <span>Expires (hours)</span>
              <input
                type="number"
                bind:value={inviteExpiryHours}
                placeholder="never"
                min="1"
                disabled={inviteLoading}
              />
            </label>
          </div>
          <button type="submit" disabled={inviteLoading}>
            {inviteLoading ? 'Creating...' : 'Create Invite'}
          </button>
        </form>

        {#if invites.length > 0}
          <div class="invite-list">
            {#each invites as { invite, acceptors }}
              {@const dead = invite.revoked || isExpired(invite) || isUsedUp(invite)}
              <div class="invite-item" class:dead>
                <div class="invite-header">
                  <span class="invite-label">
                    {invite.label || 'Untitled invite'}
                  </span>
                  <span class="invite-usage">
                    {invite.use_count}{invite.max_uses !== null ? `/${invite.max_uses}` : ''} used
                  </span>
                </div>
                <div class="invite-meta">
                  <span>{formatDate(invite.created_at)}</span>
                  {#if invite.revoked}
                    <span class="badge revoked">Revoked</span>
                  {:else if isExpired(invite)}
                    <span class="badge expired">Expired</span>
                  {:else if isUsedUp(invite)}
                    <span class="badge used-up">Used up</span>
                  {:else}
                    <span class="badge active">Active</span>
                  {/if}
                </div>
                {#if !dead}
                  <div class="invite-actions">
                    <button class="small-btn" onclick={() => copyInviteLink(invite.token)}>
                      {copiedToken === invite.token ? 'Copied!' : 'Copy link'}
                    </button>
                    <button class="small-btn danger" onclick={() => revokeInvite(invite.token)}> Revoke </button>
                  </div>
                {/if}
                {#if acceptors.length > 0}
                  <div class="acceptors">
                    {#each acceptors as acceptor}
                      <span class="acceptor">{acceptor.display_name}</span>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {/if}

      <hr />

      <button class="logout-btn" onclick={handleLogout}>Sign Out</button>
    {/if}

    <p class="footer-link">
      <a href="{base}/">Back to dashboard</a>
    </p>
  </div>
</div>

<style>
  .auth-page {
    position: fixed;
    inset: 0;
    overflow-y: auto;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    background: var(--surface-900);
    padding: 40px 20px;
  }

  .auth-card {
    width: 100%;
    max-width: 420px;
    padding: 32px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 8px;
  }

  .back-link {
    display: inline-block;
    margin-bottom: 12px;
    font-size: 0.8em;
    color: var(--text-muted);
    text-decoration: none;
  }

  .back-link:hover {
    color: var(--chrome-accent-400);
  }

  h1 {
    margin: 0 0 20px;
    font-size: 1.4em;
    color: var(--chrome-accent-400);
    text-align: center;
  }

  h2 {
    margin: 0 0 16px;
    font-size: 1em;
    color: var(--text-primary);
  }

  .section-desc {
    margin: -8px 0 16px;
    font-size: 0.8em;
    color: var(--text-muted);
  }

  .user-info {
    display: flex;
    justify-content: space-between;
    padding: 6px 0;
    font-size: 0.85em;
  }

  .user-info .label {
    color: var(--text-muted);
  }

  .user-info .value {
    color: var(--text-primary);
  }

  .admin-badge {
    display: inline-block;
    margin-top: 8px;
    padding: 2px 8px;
    background: var(--tint-active-strong);
    border: 1px solid var(--tint-selection);
    border-radius: 3px;
    color: var(--chrome-accent-400);
    font-size: 0.75em;
    font-weight: 600;
    letter-spacing: 0.05em;
  }

  hr {
    border: none;
    border-top: 1px solid var(--surface-border);
    margin: 20px 0;
  }

  .error {
    padding: 8px 12px;
    margin-bottom: 16px;
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-border);
    border-radius: 4px;
    color: var(--status-red);
    font-size: 0.85em;
  }

  .success {
    padding: 8px 12px;
    margin-bottom: 16px;
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-border);
    border-radius: 4px;
    color: var(--status-green-text);
    font-size: 0.85em;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  label {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  label span {
    font-size: 0.8em;
    color: var(--text-secondary);
  }

  input {
    padding: 8px 12px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 0.9em;
    width: 100%;
    box-sizing: border-box;
  }

  input:focus {
    outline: none;
    border-color: var(--chrome-accent-600);
  }

  button[type='submit'] {
    padding: 10px;
    background: var(--chrome-accent-600);
    border: none;
    border-radius: 4px;
    color: var(--surface-900);
    font-family: inherit;
    font-weight: 600;
    font-size: 0.9em;
    cursor: pointer;
    margin-top: 8px;
  }

  button[type='submit']:hover:not(:disabled) {
    background: var(--chrome-accent-500);
  }

  button[type='submit']:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .logout-btn {
    width: 100%;
    padding: 10px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 0.85em;
    cursor: pointer;
  }

  .logout-btn:hover {
    border-color: var(--status-red);
    color: var(--status-red);
  }

  .footer-link {
    margin-top: 16px;
    text-align: center;
    font-size: 0.8em;
    color: var(--text-muted);
  }

  .footer-link a {
    color: var(--chrome-accent-400);
    text-decoration: none;
  }

  .footer-link a:hover {
    text-decoration: underline;
  }

  /* Invite-specific styles */

  .invite-form {
    margin-bottom: 16px;
  }

  .invite-row {
    display: flex;
    gap: 12px;
  }

  .invite-row label.compact {
    flex: 1;
    min-width: 0;
  }

  .invite-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 16px;
  }

  .invite-item {
    padding: 12px;
    background: var(--surface-800);
    border: 1px solid var(--surface-border);
    border-radius: 6px;
  }

  .invite-item.dead {
    opacity: 0.55;
  }

  .invite-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 4px;
  }

  .invite-label {
    font-size: 0.85em;
    color: var(--text-primary);
    font-weight: 500;
  }

  .invite-usage {
    font-size: 0.75em;
    color: var(--text-muted);
  }

  .invite-meta {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 0.75em;
    color: var(--text-muted);
    margin-bottom: 8px;
  }

  .badge {
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 0.85em;
    font-weight: 600;
  }

  .badge.active {
    background: var(--status-green-tint);
    color: var(--status-green-text);
  }

  .badge.revoked {
    background: var(--status-red-tint);
    color: var(--status-red);
  }

  .badge.expired {
    background: var(--tint-active-strong);
    color: var(--chrome-accent-500);
  }

  .badge.used-up {
    background: var(--tint-hover);
    color: var(--text-muted);
  }

  .invite-actions {
    display: flex;
    gap: 8px;
  }

  .small-btn {
    padding: 4px 10px;
    font-size: 0.75em;
    font-family: inherit;
    border-radius: 3px;
    cursor: pointer;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    color: var(--text-secondary);
  }

  .small-btn:hover {
    border-color: var(--chrome-accent-600);
    color: var(--text-primary);
  }

  .small-btn.danger:hover {
    border-color: var(--status-red);
    color: var(--status-red);
  }

  .acceptors {
    margin-top: 8px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .acceptor {
    padding: 2px 8px;
    background: var(--status-blue-tint);
    border-radius: 3px;
    font-size: 0.7em;
    color: var(--status-blue);
  }
</style>
