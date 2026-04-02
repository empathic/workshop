<script lang="ts">
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { register } from '$lib/stores/auth';
  import { onMount } from 'svelte';

  let username = $state('');
  let displayName = $state('');
  let password = $state('');
  let confirmPassword = $state('');
  let error = $state('');
  let loading = $state(false);

  let inviteToken = $state<string | null>(null);
  let inviteValid = $state<boolean | null>(null);
  let inviteLabel = $state<string | null>(null);
  let inviteChecking = $state(false);

  onMount(async () => {
    const token = $page.url.searchParams.get('invite');
    if (token) {
      inviteToken = token;
      inviteChecking = true;
      try {
        const resp = await fetch(`/api/auth/check-invite?token=${encodeURIComponent(token)}`);
        if (resp.ok) {
          const data = await resp.json();
          inviteValid = data.valid;
          inviteLabel = data.label ?? null;
        } else {
          inviteValid = false;
        }
      } catch {
        inviteValid = false;
      }
      inviteChecking = false;
    }
  });

  async function handleSubmit(e: Event) {
    e.preventDefault();
    error = '';

    if (password !== confirmPassword) {
      error = 'Passwords do not match';
      return;
    }

    loading = true;
    const result = await register(username, password, displayName || undefined, inviteToken ?? undefined);
    loading = false;

    if (result.ok) {
      goto(`${base}/`);
    } else {
      error = result.error ?? 'Registration failed';
    }
  }
</script>

<div class="auth-page">
  <div class="auth-card">
    <h1>Workshop</h1>
    <p class="subtitle">Create your account</p>

    {#if inviteChecking}
      <div class="invite-banner checking">Checking invite...</div>
    {:else if inviteValid === true}
      <div class="invite-banner valid">
        You've been invited{inviteLabel ? ` — ${inviteLabel}` : ''}
      </div>
    {:else if inviteValid === false}
      <div class="invite-banner invalid">This invite link is invalid or has expired.</div>
    {/if}

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <form onsubmit={handleSubmit}>
      <label>
        <span>Username</span>
        <input
          type="text"
          bind:value={username}
          required
          minlength="2"
          maxlength="64"
          autocomplete="username"
          disabled={loading || inviteValid === false}
        />
      </label>

      <label>
        <span>Display Name (optional)</span>
        <input
          type="text"
          bind:value={displayName}
          placeholder={username || 'Your name'}
          autocomplete="name"
          disabled={loading || inviteValid === false}
        />
      </label>

      <label>
        <span>Password</span>
        <input
          type="password"
          bind:value={password}
          required
          minlength="8"
          autocomplete="new-password"
          disabled={loading || inviteValid === false}
        />
      </label>

      <label>
        <span>Confirm Password</span>
        <input
          type="password"
          bind:value={confirmPassword}
          required
          minlength="8"
          autocomplete="new-password"
          disabled={loading || inviteValid === false}
        />
      </label>

      <button type="submit" disabled={loading || inviteValid === false}>
        {loading ? 'Creating account...' : 'Create Account'}
      </button>
    </form>

    <p class="footer-link">
      Already have an account? <a href="{base}/login">Sign in</a>
    </p>
  </div>
</div>

<style>
  .auth-page {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--surface-900);
  }

  .auth-card {
    width: 100%;
    max-width: 360px;
    padding: 32px;
    background: var(--surface-700);
    border: 1px solid var(--surface-border);
    border-radius: 8px;
  }

  h1 {
    margin: 0 0 4px;
    font-size: 1.4em;
    color: var(--chrome-accent-400);
    text-align: center;
  }

  .subtitle {
    margin: 0 0 24px;
    color: var(--text-muted);
    text-align: center;
    font-size: 0.85em;
  }

  .invite-banner {
    padding: 8px 12px;
    margin-bottom: 16px;
    border-radius: 4px;
    font-size: 0.85em;
    text-align: center;
  }

  .invite-banner.valid {
    background: var(--status-green-tint);
    border: 1px solid var(--status-green-border);
    color: var(--status-green-text);
  }

  .invite-banner.invalid {
    background: var(--status-red-tint);
    border: 1px solid var(--status-red-border);
    color: var(--status-red);
  }

  .invite-banner.checking {
    background: var(--status-blue-tint);
    border: 1px solid var(--status-blue-tint);
    color: var(--status-blue);
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
  }

  input:focus {
    outline: none;
    border-color: var(--chrome-accent-600);
  }

  input::placeholder {
    color: var(--text-muted);
  }

  button {
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

  button:hover:not(:disabled) {
    background: var(--chrome-accent-500);
  }

  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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
</style>
