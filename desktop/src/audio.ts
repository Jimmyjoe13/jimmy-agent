/**
 * Capture micro dans la fenêtre, pour les tests et la dictée manuelle.
 *
 * Pourquoi le webview et pas Rust ? Parce que l'écoute permanente (wake word)
 * se fait en Rust avec `cpal` — plus fiable et sans dépendre du WebView. Mais
 * pour * tester* la reconnaissance vocale depuis l'interface, un
 * enregistreur de trois lignes dans la fenêtre est beaucoup plus simple, et
 * produit exactement le WAV 16 kHz que whisper.cpp attend.
 */

const TARGET_RATE = 16000;

export interface RecordingResult {
  /** WAV PCM 16 bits mono 16 kHz. */
  wav: number[];
  durationMs: number;
}

export class Recorder {
  private context: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private processor: ScriptProcessorNode | null = null;
  private source: MediaStreamAudioSourceNode | null = null;
  private chunks: Float32Array[] = [];
  private level = 0;

  async start(): Promise<void> {
    if (this.context) return;
    this.stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      },
    });
    const context = new AudioContext();
    this.context = context;
    const source = context.createMediaStreamSource(this.stream);
    this.source = source;
    // ScriptProcessor : déprécié mais universellement supporté, et un
    // AudioWorklet exigerait un fichier séparé chargé par URL.
    const processor = context.createScriptProcessor(4096, 1, 1);
    this.processor = processor;
    processor.onaudioprocess = (event) => {
      const input = event.inputBuffer.getChannelData(0);
      this.chunks.push(new Float32Array(input));
      let sum = 0;
      for (let i = 0; i < input.length; i += 1) sum += input[i] * input[i];
      this.level = Math.sqrt(sum / input.length);
    };
    source.connect(processor);
    // Sans sortie connectée, certains navigateurs suspendent le graphe.
    const silent = context.createGain();
    silent.gain.value = 0;
    processor.connect(silent);
    silent.connect(context.destination);
  }

  get active(): boolean {
    return this.context !== null;
  }

  /** Niveau courant, 0 → 1, pour la jauge. */
  getLevel(): number {
    return Math.min(1, this.level * 8);
  }

  async stop(): Promise<RecordingResult> {
    const context = this.context;
    if (!context) return { wav: [], durationMs: 0 };
    this.processor?.disconnect();
    this.source?.disconnect();
    this.stream?.getTracks().forEach((track) => track.stop());
    await context.close();

    const total = this.chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    const merged = new Float32Array(total);
    let offset = 0;
    for (const chunk of this.chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    this.chunks = [];
    this.context = null;
    this.processor = null;
    this.source = null;
    this.stream = null;

    const resampled = resample(merged, context.sampleRate, TARGET_RATE);
    return {
      wav: encodeWav(resampled, TARGET_RATE),
      durationMs: Math.round((merged.length / context.sampleRate) * 1000),
    };
  }
}

/** Rééchantillonnage linéaire — même algorithme que le côté Rust. */
function resample(input: Float32Array, from: number, to: number): Float32Array {
  if (from === to || input.length === 0) return input;
  const ratio = to / from;
  const length = Math.round(input.length * ratio);
  const out = new Float32Array(length);
  for (let i = 0; i < length; i += 1) {
    const position = i / ratio;
    const index = Math.floor(position);
    const frac = position - index;
    const a = input[index] ?? 0;
    const b = input[index + 1] ?? a;
    out[i] = a + (b - a) * frac;
  }
  return out;
}

/** Construit un fichier WAV PCM 16 bits mono. */
function encodeWav(samples: Float32Array, sampleRate: number): number[] {
  const bytes = new ArrayBuffer(44 + samples.length * 2);
  const view = new DataView(bytes);
  const writeString = (offset: number, text: string) => {
    for (let i = 0; i < text.length; i += 1) view.setUint8(offset + i, text.charCodeAt(i));
  };
  writeString(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  writeString(8, "WAVE");
  writeString(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeString(36, "data");
  view.setUint32(40, samples.length * 2, true);
  let offset = 44;
  for (let i = 0; i < samples.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, clamped * 32767, true);
    offset += 2;
  }
  return Array.from(new Uint8Array(bytes));
}