import {
  updateVoiceMetrics,
  recordVoiceTranscription,
  recordVoiceError,
  voiceBackendOverride
} from '$lib/stores/metrics';
import { get } from 'svelte/store';

export type VoiceBackend = 'hybrid' | 'prompt-api' | 'web-speech' | 'none';

export interface VoiceSessionCallbacks {
  onInterim: (text: string) => void;
  onFinal: (text: string) => void;
  onError: (err: string) => void;
  onStateChange: (state: 'listening' | 'transcribing' | 'idle') => void;
  onVolumeChange?: (level: number) => void;
  onFrequencyData?: (data: Uint8Array) => void;
}

export interface VoiceSession {
  start(): void;
  stop(): void;
  destroy(): void;
  backend: VoiceBackend;
}

export async function detectVoiceBackend(): Promise<VoiceBackend> {
  // If an override is set, return it directly (skip detection)
  const override = get(voiceBackendOverride);
  if (override) {
    console.debug('[voice] Using override backend:', override);
    return override;
  }

  let hasPromptApi = false;

  // Check Prompt API with audio support
  if (typeof LanguageModel !== 'undefined') {
    try {
      const availability: string = await LanguageModel.availability({
        expectedInputs: [{ type: 'audio' }]
      });
      console.debug('[voice] Prompt API audio availability:', availability);
      if (availability !== 'unavailable' && availability !== 'no') {
        hasPromptApi = true;
      }
    } catch (err) {
      console.debug('[voice] Prompt API audio check failed:', err);
    }
  }

  // Check Web Speech API
  const SpeechRecognitionCtor =
    typeof window !== 'undefined' ? window.SpeechRecognition || window.webkitSpeechRecognition : undefined;
  const hasWebSpeech = !!SpeechRecognitionCtor;

  // Both available: hybrid gives live interims + corrected final
  const backend: VoiceBackend =
    hasPromptApi && hasWebSpeech ? 'hybrid' : hasPromptApi ? 'prompt-api' : hasWebSpeech ? 'web-speech' : 'none';
  console.debug('[voice] Detected backend:', backend, { hasPromptApi, hasWebSpeech });
  return backend;
}

/** Returns which backends the current browser supports (for UI display). */
export async function availableVoiceBackends(): Promise<VoiceBackend[]> {
  const backends: VoiceBackend[] = [];

  let hasPromptApi = false;
  if (typeof LanguageModel !== 'undefined') {
    try {
      const availability: string = await LanguageModel.availability({
        expectedInputs: [{ type: 'audio' }]
      });
      if (availability !== 'unavailable' && availability !== 'no') {
        hasPromptApi = true;
      }
    } catch {
      // not available
    }
  }

  const hasWebSpeech = !!(
    typeof window !== 'undefined' &&
    (window.SpeechRecognition || window.webkitSpeechRecognition)
  );

  if (hasPromptApi && hasWebSpeech) backends.push('hybrid');
  if (hasPromptApi) backends.push('prompt-api');
  if (hasWebSpeech) backends.push('web-speech');

  return backends;
}

export function createVoiceSession(backend: VoiceBackend, callbacks: VoiceSessionCallbacks): VoiceSession {
  updateVoiceMetrics({ backend });
  if (backend === 'hybrid') {
    return createHybridSession(callbacks);
  }
  if (backend === 'prompt-api') {
    return createPromptApiSession(callbacks);
  }
  if (backend === 'web-speech') {
    return createWebSpeechSession(callbacks);
  }
  return { start() {}, stop() {}, destroy() {}, backend: 'none' };
}

// ---------------------------------------------------------------------------
// Volume analyser — AnalyserNode → RMS → 0-1 level via RAF
// ---------------------------------------------------------------------------

interface VolumeAnalyser {
  destroy(): void;
}

function createVolumeAnalyser(
  stream: MediaStream,
  onLevel: (n: number) => void,
  onFrequencyData?: (data: Uint8Array) => void
): VolumeAnalyser {
  const audioCtx = new AudioContext();
  const source = audioCtx.createMediaStreamSource(stream);
  const analyser = audioCtx.createAnalyser();
  analyser.fftSize = 512;
  analyser.smoothingTimeConstant = 0.6;
  source.connect(analyser);

  const buffer = new Uint8Array(analyser.fftSize);
  const freqBuffer = new Uint8Array(analyser.frequencyBinCount);
  let rafId = 0;
  let alive = true;

  function tick() {
    if (!alive) return;
    analyser.getByteTimeDomainData(buffer);
    // Compute RMS
    let sum = 0;
    for (let i = 0; i < buffer.length; i++) {
      const v = (buffer[i]! - 128) / 128;
      sum += v * v;
    }
    const rms = Math.sqrt(sum / buffer.length);
    // Normalize to 0-1 (speech RMS rarely exceeds ~0.5)
    onLevel(Math.min(1, rms * 3));

    if (onFrequencyData) {
      analyser.getByteFrequencyData(freqBuffer);
      onFrequencyData(freqBuffer);
    }

    rafId = requestAnimationFrame(tick);
  }
  rafId = requestAnimationFrame(tick);

  return {
    destroy() {
      alive = false;
      cancelAnimationFrame(rafId);
      source.disconnect();
      audioCtx.close().catch(() => {});
    }
  };
}

// ---------------------------------------------------------------------------
// Prompt API backend — records audio, then batch-transcribes via Gemini Nano
// ---------------------------------------------------------------------------

function createPromptApiSession(cb: VoiceSessionCallbacks): VoiceSession {
  let session: LanguageModel | null = null;
  let sessionPromise: Promise<LanguageModel | null> | null = null;
  let mediaStream: MediaStream | null = null;
  let recorder: MediaRecorder | null = null;
  let volumeAnalyser: VolumeAnalyser | null = null;
  let chunks: Blob[] = [];
  let active = false;
  let destroyed = false;
  let transcribeStart = 0;

  // Create session lazily (must be called within a user gesture for downloadable models)
  function ensureSession(): Promise<LanguageModel | null> {
    if (session) return Promise.resolve(session);
    if (sessionPromise) return sessionPromise;
    sessionPromise = LanguageModel.create({ expectedInputs: [{ type: 'audio' }] })
      .then((s: LanguageModel) => {
        if (destroyed) {
          s.destroy();
          return null;
        }
        session = s;
        return s;
      })
      .catch((err: unknown) => {
        const msg = `Failed to create Prompt API session: ${err}`;
        cb.onError(msg);
        recordVoiceError(msg);
        sessionPromise = null;
        return null;
      });
    return sessionPromise!;
  }

  return {
    backend: 'prompt-api',

    start() {
      if (active || destroyed) return;
      active = true;
      chunks = [];

      // Kick off session creation inside user gesture
      ensureSession();

      navigator.mediaDevices
        .getUserMedia({ audio: true })
        .then((stream) => {
          if (!active || destroyed) {
            stream.getTracks().forEach((t) => t.stop());
            return;
          }
          mediaStream = stream;
          recorder = new MediaRecorder(stream);
          recorder.ondataavailable = (e) => {
            if (e.data.size > 0) chunks.push(e.data);
          };
          recorder.start();
          if (cb.onVolumeChange) {
            volumeAnalyser = createVolumeAnalyser(stream, cb.onVolumeChange, cb.onFrequencyData);
          }
          cb.onStateChange('listening');
          updateVoiceMetrics({ state: 'listening' });
        })
        .catch((err) => {
          active = false;
          const msg = `Microphone access denied: ${err}`;
          cb.onError(msg);
          cb.onStateChange('idle');
          recordVoiceError(msg);
        });
    },

    stop() {
      if (!active || destroyed) return;
      active = false;
      volumeAnalyser?.destroy();
      volumeAnalyser = null;

      if (recorder && recorder.state !== 'inactive') {
        recorder.onstop = async () => {
          // Stop mic tracks
          mediaStream?.getTracks().forEach((t) => t.stop());
          mediaStream = null;
          recorder = null;

          if (chunks.length === 0) {
            cb.onStateChange('idle');
            updateVoiceMetrics({ state: 'idle' });
            return;
          }

          cb.onStateChange('transcribing');
          updateVoiceMetrics({ state: 'transcribing' });
          transcribeStart = performance.now();

          try {
            const blob = new Blob(chunks, { type: 'audio/webm' });
            const arrayBuffer = await blob.arrayBuffer();
            const audioCtx = new AudioContext();
            const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
            await audioCtx.close();

            const s = await ensureSession();
            if (!s) {
              throw new Error('Prompt API session failed to initialize');
            }

            const response = await s.prompt([
              {
                role: 'user',
                content: [
                  {
                    type: 'text',
                    value:
                      'Transcribe the following audio exactly as spoken. Output only the transcribed text, nothing else.'
                  },
                  { type: 'audio', value: audioBuffer }
                ]
              }
            ]);

            const elapsed = Math.round(performance.now() - transcribeStart);
            cb.onFinal(response.trim());
            recordVoiceTranscription(elapsed);
          } catch (err) {
            const msg = `Transcription failed: ${err}`;
            cb.onError(msg);
            recordVoiceError(msg);
          } finally {
            cb.onStateChange('idle');
            updateVoiceMetrics({ state: 'idle' });
          }
        };
        recorder.stop();
      } else {
        mediaStream?.getTracks().forEach((t) => t.stop());
        mediaStream = null;
        recorder = null;
        cb.onStateChange('idle');
        updateVoiceMetrics({ state: 'idle' });
      }
    },

    destroy() {
      destroyed = true;
      active = false;
      volumeAnalyser?.destroy();
      volumeAnalyser = null;
      if (recorder && recorder.state !== 'inactive') {
        recorder.stop();
      }
      mediaStream?.getTracks().forEach((t) => t.stop());
      mediaStream = null;
      recorder = null;
      session?.destroy();
      session = null;
    }
  };
}

// ---------------------------------------------------------------------------
// Hybrid backend — Web Speech interims + Prompt API final correction
// ---------------------------------------------------------------------------

function createHybridSession(cb: VoiceSessionCallbacks): VoiceSession {
  const SpeechRecognitionCtor = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRecognitionCtor) throw new Error('Web Speech API not available');

  const recognition = new SpeechRecognitionCtor();
  recognition.continuous = true;
  recognition.interimResults = true;
  recognition.lang = 'en-US';

  let session: LanguageModel | null = null;
  let sessionPromise: Promise<LanguageModel | null> | null = null;
  let mediaStream: MediaStream | null = null;
  let recorder: MediaRecorder | null = null;
  let volumeAnalyser: VolumeAnalyser | null = null;
  let chunks: Blob[] = [];
  let active = false;
  let destroyed = false;
  let transcribeStart = 0;

  function ensureSession(): Promise<LanguageModel | null> {
    if (session) return Promise.resolve(session);
    if (sessionPromise) return sessionPromise;
    sessionPromise = LanguageModel.create({ expectedInputs: [{ type: 'audio' }] })
      .then((s: LanguageModel) => {
        if (destroyed) {
          s.destroy();
          return null;
        }
        session = s;
        return s;
      })
      .catch((err: unknown) => {
        const msg = `Failed to create Prompt API session: ${err}`;
        cb.onError(msg);
        recordVoiceError(msg);
        sessionPromise = null;
        return null;
      });
    return sessionPromise!;
  }

  // Wire up Web Speech callbacks for live streaming.
  // Always emit as interim — in hybrid mode, Web Speech results are drafts.
  // Only the Prompt API correction produces the true final transcript.
  recognition.onresult = (event: SpeechRecognitionEvent) => {
    const transcript = Array.from(event.results)
      .map((result) => result[0].transcript)
      .join(' ');

    cb.onInterim(transcript);
  };

  recognition.onerror = (event: SpeechRecognitionErrorEvent) => {
    // no-speech and aborted are normal during pauses / on stop — not real errors
    if (event.error === 'no-speech' || event.error === 'aborted') {
      console.debug('[voice] Speech recognition:', event.error);
      return;
    }
    console.error('Speech recognition error:', event.error);
    cb.onError(event.error);
    recordVoiceError(event.error);
  };

  recognition.onstart = () => {
    console.debug('[voice] recognition.onstart fired — recognition is active');
  };

  // Chrome fires onend even with continuous=true (e.g. after no-speech timeout).
  // Restart recognition automatically while still actively listening.
  recognition.onend = () => {
    console.debug('[voice] recognition.onend fired, active:', active);
    if (active && !destroyed) {
      try {
        recognition.start();
      } catch {
        // already started — ignore
      }
    }
  };

  return {
    backend: 'hybrid',

    start() {
      if (active || destroyed) return;
      active = true;
      chunks = [];

      // Kick off Prompt API session creation inside user gesture
      ensureSession();

      // Start recognition SYNCHRONOUSLY — must be in user gesture context.
      // Chrome silently ignores SpeechRecognition.start() without user activation.
      try {
        recognition.start();
      } catch {
        // already started — ignore
      }
      cb.onStateChange('listening');
      updateVoiceMetrics({ state: 'listening' });

      // Acquire mic async for MediaRecorder (Prompt API correction).
      // If getUserMedia causes Chrome to kill recognition (audio re-routing),
      // the onend handler will auto-restart it once audio routing is stable.
      navigator.mediaDevices
        .getUserMedia({ audio: true })
        .then((stream) => {
          if (!active || destroyed) {
            stream.getTracks().forEach((t) => t.stop());
            return;
          }
          mediaStream = stream;
          recorder = new MediaRecorder(stream);
          recorder.ondataavailable = (e) => {
            if (e.data.size > 0) chunks.push(e.data);
          };
          recorder.start();
          if (cb.onVolumeChange) {
            volumeAnalyser = createVolumeAnalyser(stream, cb.onVolumeChange, cb.onFrequencyData);
          }
        })
        .catch((err) => {
          // Mic denied — recognition may still work (uses its own mic).
          // Log but don't kill the session.
          console.warn('[voice] getUserMedia failed, correction unavailable:', err);
        });
    },

    stop() {
      if (!active || destroyed) return;
      active = false;
      volumeAnalyser?.destroy();
      volumeAnalyser = null;

      // Stop Web Speech
      recognition.stop();

      // Stop MediaRecorder and send to Prompt API for correction
      if (recorder && recorder.state !== 'inactive') {
        recorder.onstop = async () => {
          mediaStream?.getTracks().forEach((t) => t.stop());
          mediaStream = null;
          recorder = null;

          if (chunks.length === 0) {
            cb.onStateChange('idle');
            updateVoiceMetrics({ state: 'idle' });
            return;
          }

          cb.onStateChange('transcribing');
          updateVoiceMetrics({ state: 'transcribing' });
          transcribeStart = performance.now();

          try {
            const blob = new Blob(chunks, { type: 'audio/webm' });
            const arrayBuffer = await blob.arrayBuffer();
            const audioCtx = new AudioContext();
            const audioBuffer = await audioCtx.decodeAudioData(arrayBuffer);
            await audioCtx.close();

            const s = await ensureSession();
            if (!s) {
              throw new Error('Prompt API session failed to initialize');
            }

            const response = await s.prompt([
              {
                role: 'user',
                content: [
                  {
                    type: 'text',
                    value:
                      'Transcribe the following audio exactly as spoken. Output only the transcribed text, nothing else.'
                  },
                  { type: 'audio', value: audioBuffer }
                ]
              }
            ]);

            const elapsed = Math.round(performance.now() - transcribeStart);
            cb.onFinal(response.trim());
            recordVoiceTranscription(elapsed);
          } catch (err) {
            // Correction failed — Web Speech result stands
            const msg = `Hybrid correction failed: ${err}`;
            cb.onError(msg);
            recordVoiceError(msg);
          } finally {
            cb.onStateChange('idle');
            updateVoiceMetrics({ state: 'idle' });
          }
        };
        recorder.stop();
      } else {
        // No recorder — just clean up
        mediaStream?.getTracks().forEach((t) => t.stop());
        mediaStream = null;
        recorder = null;
        cb.onStateChange('idle');
        updateVoiceMetrics({ state: 'idle' });
      }
    },

    destroy() {
      destroyed = true;
      active = false;
      volumeAnalyser?.destroy();
      volumeAnalyser = null;
      recognition.abort();
      if (recorder && recorder.state !== 'inactive') {
        recorder.stop();
      }
      mediaStream?.getTracks().forEach((t) => t.stop());
      mediaStream = null;
      recorder = null;
      session?.destroy();
      session = null;
    }
  };
}

// ---------------------------------------------------------------------------
// Web Speech API backend — streaming recognition via browser/OS speech engine
// ---------------------------------------------------------------------------

function createWebSpeechSession(cb: VoiceSessionCallbacks): VoiceSession {
  const SpeechRecognitionCtor = window.SpeechRecognition || window.webkitSpeechRecognition;
  if (!SpeechRecognitionCtor) throw new Error('Web Speech API not available');
  const recognition = new SpeechRecognitionCtor();
  recognition.continuous = true;
  recognition.interimResults = true;
  recognition.lang = 'en-US';

  let mediaStream: MediaStream | null = null;
  let volumeAnalyser: VolumeAnalyser | null = null;

  recognition.onresult = (event: SpeechRecognitionEvent) => {
    const transcript = Array.from(event.results)
      .map((result) => result[0].transcript)
      .join(' ');

    const lastResult = event.results[event.results.length - 1];
    if (lastResult.isFinal) {
      cb.onFinal(transcript);
      recordVoiceTranscription();
    } else {
      cb.onInterim(transcript);
    }
  };

  recognition.onerror = (event: SpeechRecognitionErrorEvent) => {
    if (event.error === 'no-speech' || event.error === 'aborted') {
      console.debug('[voice] Speech recognition:', event.error);
      return;
    }
    console.error('Speech recognition error:', event.error);
    cb.onError(event.error);
    cb.onStateChange('idle');
    recordVoiceError(event.error);
  };

  function cleanupMic() {
    volumeAnalyser?.destroy();
    volumeAnalyser = null;
    mediaStream?.getTracks().forEach((t) => t.stop());
    mediaStream = null;
  }

  recognition.onend = () => {
    cleanupMic();
    cb.onStateChange('idle');
    updateVoiceMetrics({ state: 'idle' });
  };

  return {
    backend: 'web-speech',

    start() {
      recognition.start();
      cb.onStateChange('listening');
      updateVoiceMetrics({ state: 'listening' });

      // Acquire mic for volume meter only
      if (cb.onVolumeChange) {
        navigator.mediaDevices
          .getUserMedia({ audio: true })
          .then((stream) => {
            mediaStream = stream;
            volumeAnalyser = createVolumeAnalyser(stream, cb.onVolumeChange!, cb.onFrequencyData);
          })
          .catch(() => {
            // Volume meter unavailable — not critical
          });
      }
    },

    stop() {
      cleanupMic();
      recognition.stop();
      // onend callback will fire onStateChange('idle')
    },

    destroy() {
      cleanupMic();
      recognition.abort();
    }
  };
}
