<script lang="ts">
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { page } from '$app/stores';
  import { login } from '$lib/stores/auth';

  let username = $state('');
  let password = $state('');
  let error = $state('');
  let loading = $state(false);

  async function handleSubmit(e: Event) {
    e.preventDefault();
    error = '';
    loading = true;

    const result = await login(username, password);
    loading = false;

    if (result.ok) {
      const redirect = $page.url.searchParams.get('redirect');
      goto(redirect || `${base}/`);
    } else {
      error = result.error ?? 'Login failed';
    }
  }
</script>

<div class="auth-page">
  <div class="auth-card">
    <h1>Workshop</h1>
    <p class="subtitle">Sign in to continue</p>

    {#if error}
      <div class="error">{error}</div>
    {/if}

    <form onsubmit={handleSubmit}>
      <label>
        <span>Username</span>
        <input type="text" bind:value={username} required minlength="2" autocomplete="username" disabled={loading} />
      </label>

      <label>
        <span>Password</span>
        <input
          type="password"
          bind:value={password}
          required
          minlength="8"
          autocomplete="current-password"
          disabled={loading}
        />
      </label>

      <button type="submit" disabled={loading}>
        {loading ? 'Signing in...' : 'Sign In'}
      </button>
    </form>

    <p class="footer-link">
      New here? <a href="{base}/register">Create an account</a>
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
