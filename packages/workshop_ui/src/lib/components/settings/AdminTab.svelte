<script lang="ts">
  import { currentUser, authEnabled } from '$lib/stores/auth';
  import { api } from '$lib/utils/api';
  import { base } from '$app/paths';
  import { onMount } from 'svelte';

  // =========================================================================
  // Types
  // =========================================================================

  interface AdminUser {
    id: string;
    username: string;
    display_name: string;
    is_admin: boolean;
    is_disabled: boolean;
    created_at: number;
  }

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

  // =========================================================================
  // State
  // =========================================================================

  // Admin = authenticated admin OR loopback admin (auth disabled)
  let isAdmin = $derived($currentUser?.is_admin || !$authEnabled);

  // Users
  let users = $state<AdminUser[]>([]);
  let usersLoading = $state(true);
  let usersError = $state('');
  let showCreateUser = $state(false);
  let newUsername = $state('');
  let newDisplayName = $state('');
  let newPassword = $state('');
  let newIsAdmin = $state(false);
  let createLoading = $state(false);
  let createError = $state('');

  // Per-row mutation state
  let mutatingUsers = $state(new Set<string>());
  let mutatingInvites = $state(new Set<string>());

  // Inline confirmation state
  let confirmingAction = $state<{ id: string; action: string } | null>(null);
  let confirmTimer: ReturnType<typeof setTimeout> | undefined;

  // Invites
  let invites = $state<ServerInviteWithAcceptors[]>([]);
  let invitesLoading = $state(true);
  let invitesError = $state('');
  let inviteLabel = $state('');
  let inviteMaxUses = $state('');
  let inviteExpiryHours = $state('');
  let inviteLoading = $state(false);
  let inviteCreateError = $state('');
  let copiedToken = $state<string | null>(null);

  // =========================================================================
  // Inline confirmation
  // =========================================================================

  function requestConfirm(id: string, action: string) {
    confirmingAction = { id, action };
    clearTimeout(confirmTimer);
    confirmTimer = setTimeout(() => (confirmingAction = null), 3000);
  }

  function cancelConfirm() {
    confirmingAction = null;
    clearTimeout(confirmTimer);
  }

  function isConfirming(id: string, action: string): boolean {
    return confirmingAction?.id === id && confirmingAction?.action === action;
  }

  // =========================================================================
  // User management
  // =========================================================================

  async function loadUsers() {
    usersLoading = true;
    usersError = '';
    try {
      const resp = await api('/api/admin/users');
      if (resp.ok) {
        users = await resp.json();
      } else {
        usersError = 'Failed to load users';
      }
    } catch {
      usersError = 'Network error';
    }
    usersLoading = false;
  }

  async function handleCreateUser(e: Event) {
    e.preventDefault();
    createError = '';
    createLoading = true;

    try {
      const body: Record<string, unknown> = {
        username: newUsername.trim(),
        password: newPassword,
      };
      if (newDisplayName.trim()) body.display_name = newDisplayName.trim();
      if (newIsAdmin) body.is_admin = true;

      const resp = await api('/api/admin/users', {
        method: 'POST',
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: 'Failed to create user' }));
        createError = data.error || 'Failed to create user';
      } else {
        newUsername = '';
        newDisplayName = '';
        newPassword = '';
        newIsAdmin = false;
        showCreateUser = false;
        await loadUsers();
      }
    } catch {
      createError = 'Network error';
    }
    createLoading = false;
  }

  async function toggleAdmin(user: AdminUser) {
    const id = user.id;
    mutatingUsers = new Set([...mutatingUsers, id]);
    try {
      const resp = await api(`/api/admin/users/${encodeURIComponent(id)}`, {
        method: 'PATCH',
        body: JSON.stringify({ is_admin: !user.is_admin }),
      });
      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: `Failed to update user (${resp.status})` }));
        usersError = data.error || `Failed to update user (${resp.status})`;
      } else {
        usersError = '';
        await loadUsers();
      }
    } catch (e) {
      usersError = e instanceof Error ? e.message : 'Network error';
    } finally {
      mutatingUsers = new Set([...mutatingUsers].filter((x) => x !== id));
    }
  }

  async function toggleDisabled(user: AdminUser) {
    cancelConfirm();
    const id = user.id;
    mutatingUsers = new Set([...mutatingUsers, id]);
    try {
      const resp = await api(`/api/admin/users/${encodeURIComponent(id)}`, {
        method: 'PATCH',
        body: JSON.stringify({ is_disabled: !user.is_disabled }),
      });
      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: `Failed to update user (${resp.status})` }));
        usersError = data.error || `Failed to update user (${resp.status})`;
      } else {
        usersError = '';
        await loadUsers();
      }
    } catch (e) {
      usersError = e instanceof Error ? e.message : 'Network error';
    } finally {
      mutatingUsers = new Set([...mutatingUsers].filter((x) => x !== id));
    }
  }

  async function deleteUser(user: AdminUser) {
    cancelConfirm();
    const id = user.id;
    mutatingUsers = new Set([...mutatingUsers, id]);
    try {
      const resp = await api(`/api/admin/users/${encodeURIComponent(id)}`, {
        method: 'DELETE',
      });
      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: `Failed to delete user (${resp.status})` }));
        usersError = data.error || `Failed to delete user (${resp.status})`;
      } else {
        usersError = '';
        await loadUsers();
      }
    } catch (e) {
      usersError = e instanceof Error ? e.message : 'Network error';
    } finally {
      mutatingUsers = new Set([...mutatingUsers].filter((x) => x !== id));
    }
  }

  // =========================================================================
  // Invitations
  // =========================================================================

  async function loadInvites() {
    invitesLoading = true;
    invitesError = '';
    try {
      const resp = await api('/api/admin/invites');
      if (resp.ok) {
        invites = await resp.json();
      } else {
        invitesError = 'Failed to load invitations';
      }
    } catch {
      invitesError = 'Network error';
    }
    invitesLoading = false;
  }

  async function handleCreateInvite(e: Event) {
    e.preventDefault();
    inviteCreateError = '';
    inviteLoading = true;

    try {
      const body: Record<string, unknown> = {};
      if (inviteLabel.trim()) body.label = inviteLabel.trim();
      if (inviteMaxUses) body.max_uses = parseInt(inviteMaxUses);
      if (inviteExpiryHours) body.expires_in_hours = parseInt(inviteExpiryHours);

      const resp = await api('/api/admin/invites', {
        method: 'POST',
        body: JSON.stringify(body),
      });

      if (!resp.ok) {
        const data = await resp.json().catch(() => ({ error: 'Failed to create invite' }));
        inviteCreateError = data.error || 'Failed to create invite';
      } else {
        inviteLabel = '';
        inviteMaxUses = '';
        inviteExpiryHours = '';
        await loadInvites();
      }
    } catch {
      inviteCreateError = 'Network error';
    }
    inviteLoading = false;
  }

  async function revokeInvite(token: string) {
    cancelConfirm();
    mutatingInvites = new Set([...mutatingInvites, token]);
    try {
      const resp = await api(`/api/admin/invites/${encodeURIComponent(token)}`, {
        method: 'DELETE',
      });
      if (!resp.ok) {
        invitesError = `Failed to revoke invite (${resp.status})`;
      } else {
        invitesError = '';
        await loadInvites();
      }
    } catch (e) {
      invitesError = e instanceof Error ? e.message : 'Network error';
    } finally {
      mutatingInvites = new Set([...mutatingInvites].filter((x) => x !== token));
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

  // =========================================================================
  // Helpers
  // =========================================================================

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  }

  function isExpired(invite: ServerInvite): boolean {
    return invite.expires_at !== null && Date.now() / 1000 > invite.expires_at;
  }

  function isUsedUp(invite: ServerInvite): boolean {
    return invite.max_uses !== null && invite.use_count >= invite.max_uses;
  }

  /** Check if this user is "us" (the current session user). */
  function isSelf(user: AdminUser): boolean {
    return !!$currentUser && $currentUser.id === user.id;
  }

  /** True when the sole admin in the list is this user — UI hint for last-admin. */
  let activeAdminCount = $derived(users.filter((u) => u.is_admin && !u.is_disabled).length);

  function isLastAdmin(user: AdminUser): boolean {
    return user.is_admin && !user.is_disabled && activeAdminCount <= 1;
  }

  // =========================================================================
  // Lifecycle
  // =========================================================================

  onMount(() => {
    loadUsers();
    loadInvites();
  });
</script>

{#if isAdmin}
  {#if !$authEnabled}
    <div class="loopback-notice">
      Auth is disabled — you have admin access via loopback bypass. User management changes are
      stored but not enforced until auth is enabled.
    </div>
  {/if}

  <!-- User Management -->
  <section class="settings-section">
    <h2 class="section-header">USERS</h2>

    {#if usersError}
      <div class="feedback-msg error-msg">
        {usersError}
        <button class="dismiss-btn" onclick={() => (usersError = '')}>Dismiss</button>
      </div>
    {/if}

    {#if usersLoading}
      <p class="muted-text">Loading users...</p>
    {:else if users.length === 0}
      <p class="muted-text">No user accounts yet. Create one to get started.</p>
    {:else}
      <div class="user-list">
        {#each users as user (user.id)}
          {@const busy = mutatingUsers.has(user.id)}
          <div class="user-item" class:disabled={user.is_disabled}>
            <div class="user-info">
              <span class="user-name">{user.display_name}</span>
              <span class="user-username">@{user.username}</span>
              {#if user.is_admin}
                <span class="badge admin">Admin</span>
              {/if}
              {#if user.is_disabled}
                <span class="badge disabled-badge">Disabled</span>
              {/if}
              {#if isSelf(user)}
                <span class="badge you-badge">You</span>
              {/if}
            </div>
            <div class="user-meta">
              <span>{formatDate(user.created_at)}</span>
            </div>
            {#if !isSelf(user)}
              <div class="user-actions">
                {#if isLastAdmin(user)}
                  <span class="last-admin-hint">Last admin</span>
                {:else}
                  <button
                    class="small-btn"
                    disabled={busy}
                    onclick={() => toggleAdmin(user)}
                  >
                    {busy ? '...' : user.is_admin ? 'Remove admin' : 'Make admin'}
                  </button>
                {/if}

                <!-- Disable/Enable with confirmation for disabling -->
                {#if user.is_disabled}
                  <button
                    class="small-btn"
                    disabled={busy}
                    onclick={() => toggleDisabled(user)}
                  >
                    {busy ? '...' : 'Enable'}
                  </button>
                {:else if isConfirming(user.id, 'disable')}
                  <button
                    class="small-btn confirm-btn"
                    disabled={busy}
                    onclick={() => toggleDisabled(user)}
                  >
                    Confirm
                  </button>
                  <button class="small-btn" onclick={cancelConfirm}>Cancel</button>
                {:else}
                  <button
                    class="small-btn"
                    disabled={busy}
                    onclick={() => requestConfirm(user.id, 'disable')}
                  >
                    {busy ? '...' : 'Disable'}
                  </button>
                {/if}

                <!-- Delete with confirmation -->
                {#if isConfirming(user.id, 'delete')}
                  <button
                    class="small-btn confirm-btn danger"
                    disabled={busy}
                    onclick={() => deleteUser(user)}
                  >
                    Confirm delete
                  </button>
                  <button class="small-btn" onclick={cancelConfirm}>Cancel</button>
                {:else}
                  <button
                    class="small-btn danger"
                    disabled={busy || isLastAdmin(user)}
                    onclick={() => requestConfirm(user.id, 'delete')}
                  >
                    {busy ? '...' : 'Delete'}
                  </button>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}

    {#if !showCreateUser}
      <button class="action-btn" style="margin-top: 10px" onclick={() => (showCreateUser = true)}>
        Create User
      </button>
    {:else}
      <form class="create-user-form" onsubmit={handleCreateUser}>
        {#if createError}
          <div class="feedback-msg error-msg">{createError}</div>
        {/if}
        <div class="form-field">
          <label class="setting-label" for="new-username">Username</label>
          <input
            id="new-username"
            type="text"
            class="setting-input full-width"
            bind:value={newUsername}
            placeholder="2-64 characters"
            disabled={createLoading}
            required
            minlength={2}
            maxlength={64}
          />
        </div>
        <div class="form-field">
          <label class="setting-label" for="new-display-name">Display Name</label>
          <input
            id="new-display-name"
            type="text"
            class="setting-input full-width"
            bind:value={newDisplayName}
            placeholder="defaults to username"
            disabled={createLoading}
          />
        </div>
        <div class="form-field">
          <label class="setting-label" for="new-password">Password</label>
          <input
            id="new-password"
            type="password"
            class="setting-input full-width"
            bind:value={newPassword}
            placeholder="min 8 characters"
            disabled={createLoading}
            required
            minlength={8}
          />
        </div>
        <label class="checkbox-row">
          <input type="checkbox" bind:checked={newIsAdmin} disabled={createLoading} />
          <span class="setting-label">Admin</span>
        </label>
        <div class="form-actions">
          <button class="action-btn" type="submit" disabled={createLoading}>
            {createLoading ? 'Creating...' : 'Create'}
          </button>
          <button
            class="cancel-btn"
            type="button"
            onclick={() => (showCreateUser = false)}
            disabled={createLoading}
          >
            Cancel
          </button>
        </div>
      </form>
    {/if}
  </section>

  <!-- Server Invitations -->
  <section class="settings-section">
    <h2 class="section-header">INVITATIONS</h2>
    <p class="section-desc">Create invite links so new users can self-register.</p>

    {#if invitesError}
      <div class="feedback-msg error-msg">
        {invitesError}
        <button class="dismiss-btn" onclick={() => (invitesError = '')}>Dismiss</button>
      </div>
    {/if}

    {#if inviteCreateError}
      <div class="feedback-msg error-msg">{inviteCreateError}</div>
    {/if}

    <form class="invite-form" onsubmit={handleCreateInvite}>
      <div class="form-field">
        <label class="setting-label" for="inv-label">Label</label>
        <input
          id="inv-label"
          type="text"
          class="setting-input full-width"
          bind:value={inviteLabel}
          placeholder="e.g. For Alice"
          disabled={inviteLoading}
        />
      </div>
      <div class="invite-row">
        <div class="form-field">
          <label class="setting-label" for="inv-max">Max Uses</label>
          <input
            id="inv-max"
            type="number"
            class="setting-input full-width"
            bind:value={inviteMaxUses}
            placeholder="unlimited"
            min="1"
            disabled={inviteLoading}
          />
        </div>
        <div class="form-field">
          <label class="setting-label" for="inv-expiry">Expires (hours)</label>
          <input
            id="inv-expiry"
            type="number"
            class="setting-input full-width"
            bind:value={inviteExpiryHours}
            placeholder="never"
            min="1"
            disabled={inviteLoading}
          />
        </div>
      </div>
      <button class="action-btn" type="submit" disabled={inviteLoading}>
        {inviteLoading ? 'Creating...' : 'Create Invite'}
      </button>
    </form>

    {#if invitesLoading}
      <p class="muted-text" style="margin-top: 12px">Loading invitations...</p>
    {:else if invites.length === 0}
      <p class="muted-text" style="margin-top: 12px">No invitations yet.</p>
    {:else}
      <div class="invite-list">
        {#each invites as { invite, acceptors }}
          {@const dead = invite.revoked || isExpired(invite) || isUsedUp(invite)}
          {@const busy = mutatingInvites.has(invite.token)}
          <div class="invite-item" class:dead>
            <div class="invite-header">
              <span class="invite-label">{invite.label || 'Untitled invite'}</span>
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
                <button
                  class="small-btn"
                  disabled={busy}
                  onclick={() => copyInviteLink(invite.token)}
                >
                  {copiedToken === invite.token ? 'Copied!' : 'Copy link'}
                </button>
                {#if isConfirming(invite.token, 'revoke')}
                  <button
                    class="small-btn confirm-btn danger"
                    disabled={busy}
                    onclick={() => revokeInvite(invite.token)}
                  >
                    Confirm revoke
                  </button>
                  <button class="small-btn" onclick={cancelConfirm}>Cancel</button>
                {:else}
                  <button
                    class="small-btn danger"
                    disabled={busy}
                    onclick={() => requestConfirm(invite.token, 'revoke')}
                  >
                    {busy ? '...' : 'Revoke'}
                  </button>
                {/if}
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

  .section-desc {
    font-size: 10px;
    color: var(--text-muted);
    margin: -8px 0 12px 0;
    letter-spacing: 0.02em;
  }

  .muted-text {
    font-size: 11px;
    color: var(--text-muted);
    letter-spacing: 0.02em;
    margin: 0;
  }

  .loopback-notice {
    padding: 8px 10px;
    margin-bottom: 16px;
    border-radius: 4px;
    font-size: 10px;
    letter-spacing: 0.02em;
    background: var(--tint-active);
    border: 1px solid var(--tint-active);
    color: var(--chrome-accent-400);
    line-height: 1.4;
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

  .feedback-msg {
    padding: 6px 10px;
    margin-bottom: 10px;
    border-radius: 4px;
    font-size: 11px;
    letter-spacing: 0.02em;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .error-msg {
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-tint);
    color: var(--status-red);
  }

  .dismiss-btn {
    background: none;
    border: none;
    color: inherit;
    font-family: inherit;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    opacity: 0.7;
    padding: 2px 4px;
    letter-spacing: 0.03em;
  }

  .dismiss-btn:hover {
    opacity: 1;
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

  .cancel-btn {
    padding: 6px 12px;
    background: transparent;
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    color: var(--text-muted);
    font-family: inherit;
    font-weight: 600;
    font-size: 11px;
    letter-spacing: 0.03em;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .cancel-btn:hover {
    border-color: var(--text-secondary);
    color: var(--text-secondary);
  }

  /* User list */

  .user-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .user-item {
    padding: 8px 10px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--elevation-low);
  }

  .user-item.disabled {
    opacity: 0.5;
  }

  .user-info {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .user-name {
    font-size: 11px;
    color: var(--text-primary);
    font-weight: 600;
  }

  .user-username {
    font-size: 10px;
    color: var(--text-muted);
  }

  .user-meta {
    font-size: 10px;
    color: var(--text-muted);
    margin-top: 2px;
  }

  .user-actions {
    display: flex;
    gap: 6px;
    margin-top: 6px;
    flex-wrap: wrap;
  }

  .last-admin-hint {
    font-size: 10px;
    color: var(--text-muted);
    font-style: italic;
    letter-spacing: 0.02em;
    align-self: center;
  }

  .create-user-form {
    display: flex;
    flex-direction: column;
    margin-top: 10px;
    padding: 12px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--elevation-low);
  }

  .checkbox-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 10px;
    cursor: pointer;
  }

  .checkbox-row input[type='checkbox'] {
    accent-color: var(--chrome-accent-500);
  }

  .form-actions {
    display: flex;
    gap: 8px;
  }

  /* Invitations */

  .invite-form {
    display: flex;
    flex-direction: column;
  }

  .invite-row {
    display: flex;
    gap: 10px;
  }

  .invite-row .form-field {
    flex: 1;
    min-width: 0;
  }

  .invite-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
  }

  .invite-item {
    padding: 10px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 4px;
    box-shadow: var(--elevation-low);
  }

  .invite-item.dead {
    opacity: 0.5;
  }

  .invite-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 4px;
  }

  .invite-label {
    font-size: 11px;
    color: var(--text-primary);
    font-weight: 600;
  }

  .invite-usage {
    font-size: 10px;
    color: var(--text-muted);
  }

  .invite-meta {
    display: flex;
    gap: 8px;
    align-items: center;
    font-size: 10px;
    color: var(--text-muted);
    margin-bottom: 6px;
  }

  .invite-actions {
    display: flex;
    gap: 6px;
  }

  /* Shared */

  .badge {
    padding: 1px 5px;
    border-radius: 3px;
    font-size: 9px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }

  .badge.admin {
    background: var(--tint-active);
    color: var(--chrome-accent-400);
  }

  .badge.disabled-badge {
    background: var(--tint-subtle);
    color: var(--text-muted);
  }

  .badge.you-badge {
    background: var(--tint-subtle);
    color: var(--status-blue);
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
    background: var(--tint-active);
    color: var(--chrome-accent-500);
  }

  .badge.used-up {
    background: var(--tint-subtle);
    color: var(--text-muted);
  }

  .small-btn {
    padding: 3px 8px;
    font-size: 10px;
    font-family: inherit;
    font-weight: 600;
    letter-spacing: 0.03em;
    border-radius: 3px;
    cursor: pointer;
    background: var(--surface-600);
    border: 1px solid var(--surface-border);
    color: var(--text-secondary);
    transition: all 0.15s ease;
  }

  .small-btn:hover:not(:disabled) {
    border-color: var(--chrome-accent-600);
    color: var(--text-primary);
  }

  .small-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .small-btn.danger:hover:not(:disabled) {
    border-color: var(--status-red);
    color: var(--status-red);
  }

  .small-btn.confirm-btn {
    background: var(--tint-active);
    border-color: var(--chrome-accent-500);
    color: var(--chrome-accent-400);
  }

  .small-btn.confirm-btn.danger {
    background: var(--status-red-tint);
    border-color: var(--status-red);
    color: var(--status-red);
  }

  .acceptors {
    margin-top: 6px;
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .acceptor {
    padding: 1px 6px;
    background: var(--tint-subtle);
    border-radius: 3px;
    font-size: 10px;
    color: var(--status-blue);
  }
</style>
