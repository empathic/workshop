<script lang="ts">
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { base } from '$app/paths';
  import { isAuthenticated } from '$lib/stores/auth';
  import { apiPost } from '$lib/utils/api';

  let status = $state<'accepting' | 'success' | 'error'>('accepting');
  let error = $state('');
  let instanceId = $state('');

  $effect(() => {
    const token = $page.params.token;

    if (!token) {
      status = 'error';
      error = 'No invitation token provided';
      return;
    }

    if (!$isAuthenticated) {
      // Redirect to login, preserving the invite URL to return to
      goto(`${base}/login?redirect=${base}/invite/${token}`);
      return;
    }

    acceptInvite(token);
  });

  async function acceptInvite(token: string) {
    try {
      const result = await apiPost<{ instance_id: string; role: string }>(`/api/invitations/${token}/accept`);
      instanceId = result.instance_id;
      status = 'success';

      // Auto-redirect to the instance after a brief moment
      setTimeout(() => goto(`${base}/?instance=${instanceId}`), 1500);
    } catch (e) {
      status = 'error';
      error = e instanceof Error ? e.message : 'Failed to accept invitation';
    }
  }
</script>

<div class="auth-page">
  <div class="auth-card">
    <h1>Workshop</h1>

    {#if status === 'accepting'}
      <p class="subtitle">Accepting invitation...</p>
      <div class="spinner"></div>
    {:else if status === 'success'}
      <p class="subtitle success">You've been added as a collaborator.</p>
      <p class="redirect-note">Redirecting to instance...</p>
    {:else}
      <p class="subtitle">Invitation Failed</p>
      <div class="error">{error}</div>
      <a href="{base}/" class="back-link">Go to dashboard</a>
    {/if}
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
    text-align: center;
  }

  h1 {
    margin: 0 0 4px;
    font-size: 1.4em;
    color: var(--chrome-accent-400);
  }

  .subtitle {
    margin: 0 0 24px;
    color: var(--text-muted);
    font-size: 0.85em;
  }

  .subtitle.success {
    color: var(--status-green-text);
  }

  .redirect-note {
    color: var(--text-muted);
    font-size: 0.8em;
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

  .back-link {
    display: inline-block;
    margin-top: 12px;
    color: var(--chrome-accent-400);
    text-decoration: none;
    font-size: 0.85em;
  }

  .back-link:hover {
    text-decoration: underline;
  }

  .spinner {
    width: 24px;
    height: 24px;
    border: 2px solid var(--surface-border);
    border-top-color: var(--chrome-accent-400);
    border-radius: 50%;
    margin: 0 auto;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
