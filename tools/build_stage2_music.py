#!/usr/bin/env python3
"""
Synthesizes looping high-energy crystalline synth-rock BGM for Stage 2 (*THE CRYO CONDUIT*)
and **SUB-ZERO WARDEN: BOREALIS**, encoded cleanly via `ffmpeg` to OGG Vorbis:
  - `assets/audio/music/stage2_theme.ogg` (138 BPM D-minor / Dorian icy synthwave action rock)
  - `assets/audio/music/boss_borealis.ogg` (154 BPM F#-minor relentless crystalline boss duel)
"""

from __future__ import annotations

import math
import subprocess
import tempfile
import wave
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
MUSIC_DIR = ROOT / "assets" / "audio" / "music"
SR = 44_100


def midi_to_hz(m: float) -> float:
    return 440.0 * (2.0 ** ((m - 69.0) / 12.0))


def write_ogg(stem: str, stereo: np.ndarray) -> None:
    MUSIC_DIR.mkdir(parents=True, exist_ok=True)
    x = np.tanh(stereo * 0.95)
    peak = float(np.max(np.abs(x))) if x.size else 1.0
    if peak > 1e-5:
        x = x * (0.88 / peak)
    pcm = np.clip(x * 32767.0, -32768.0, 32767.0).astype(np.int16)
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tf:
        wav_path = Path(tf.name)
    try:
        with wave.open(str(wav_path), "wb") as wf:
            wf.setnchannels(2)
            wf.setsampwidth(2)
            wf.setframerate(SR)
            wf.writeframes(pcm.tobytes())
        ogg_path = MUSIC_DIR / f"{stem}.ogg"
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", str(wav_path), "-c:a", "libvorbis", "-q:a", "5", str(ogg_path)],
            check=True,
        )
        print(f"  [BGM] {ogg_path.name} ({len(stereo)/SR:.2f}s stereo OGG)")
    finally:
        wav_path.unlink(missing_ok=True)


def synth_track(bpm: float, bars: int, bass_prog: list[int], arp_notes: list[list[int]], lead_melody: list[tuple[int, int, int]], seed: int) -> np.ndarray:
    step_sec = 60.0 / bpm / 4.0  # 16th-note grid
    total_steps = bars * 16
    total_samples = int(round(total_steps * step_sec * SR))
    left = np.zeros(total_samples, dtype=np.float64)
    right = np.zeros(total_samples, dtype=np.float64)
    rng = np.random.default_rng(seed)

    def add_seg(start_s: float, sig_l: np.ndarray, sig_r: np.ndarray) -> None:
        i0 = int(round(start_s * SR))
        n = min(len(sig_l), total_samples - i0)
        if i0 >= 0 and n > 0:
            left[i0 : i0 + n] += sig_l[:n]
            right[i0 : i0 + n] += sig_r[:n]

    # 1. Driving punchy synthwave drums + crystalline hi-hats
    for step in range(total_steps):
        t0 = step * step_sec
        in_bar = step % 16
        # Kick on 0, 6, 8, 14 (driving Mega Man X action groove!)
        if in_bar in (0, 6, 8, 14):
            n = int(SR * 0.14)
            u = np.linspace(0.0, 1.0, n)
            ph = np.cumsum(2.0 * math.pi * (145.0 * np.exp(-u * 6.5) + 46.0) / SR)
            k = np.sin(ph) * np.exp(-u * 7.0) * 0.62
            add_seg(t0, k, k)
        # Snare on 4 & 12
        if in_bar in (4, 12):
            n = int(SR * 0.16)
            u = np.linspace(0.0, 1.0, n)
            noise = rng.standard_normal(n) * np.exp(-u * 10.0) * 0.34
            body = np.sin(2.0 * math.pi * 195.0 * u * 0.16) * np.exp(-u * 15.0) * 0.30
            s = noise + body
            add_seg(t0, s * 0.95, s * 1.05)
        # Crisp 16th icy closed/open hats
        hat_dur = 0.075 if in_bar % 4 == 2 else 0.032
        n = int(SR * hat_dur)
        u = np.linspace(0.0, 1.0, n)
        h_raw = rng.standard_normal(n)
        # Simple high-pass difference
        h_hp = np.diff(h_raw, prepend=0.0) * np.exp(-u * 12.0) * (0.14 if in_bar % 2 == 0 else 0.09)
        pan_l = 0.85 if in_bar % 2 == 0 else 1.15
        pan_r = 2.0 - pan_l
        add_seg(t0, h_hp * pan_l, h_hp * pan_r)

    # 2. Rolling 16th-note saw-octave synth bassline
    for step in range(total_steps):
        bar = (step // 16) % len(bass_prog)
        root = bass_prog[bar]
        note = root + (12 if step % 4 == 2 else 0)
        f = midi_to_hz(note)
        n = int(SR * step_sec * 0.95)
        st = np.linspace(0.0, n / SR, n)
        # Rich band-limited sawtooth approximation
        saw = (
            0.55 * np.sin(2 * math.pi * f * st)
            + 0.28 * np.sin(2 * math.pi * 2 * f * st)
            + 0.16 * np.sin(2 * math.pi * 3 * f * st)
            + 0.10 * np.sin(2 * math.pi * 4 * f * st)
        )
        env = np.exp(-st * 11.0) * (1.0 - np.exp(-st * 220.0))
        b = saw * env * 0.34
        add_seg(step * step_sec, b, b)

    # 3. Shimmering stereo crystalline FM ice arpeggios!
    for step in range(total_steps):
        bar = (step // 16) % len(arp_notes)
        chord = arp_notes[bar]
        midi = chord[step % len(chord)]
        f = midi_to_hz(midi)
        n = int(SR * step_sec * 1.45)
        st = np.linspace(0.0, n / SR, n)
        fm = np.sin(2 * math.pi * f * st + 1.8 * np.exp(-st * 10.0) * np.sin(2 * math.pi * f * 2.0 * st))
        env = np.exp(-st * 8.5) * (1.0 - np.exp(-st * 240.0))
        sig = fm * env * 0.16
        pan = 0.5 + 0.38 * math.sin(step * 0.7)
        add_seg(step * step_sec, sig * (1.0 - pan), sig * pan)

    # 4. Lush cold atmospheric aurora pad chords
    for bar in range(bars):
        chord = arp_notes[bar % len(arp_notes)][:3]
        n = int(SR * step_sec * 16.0)
        st = np.linspace(0.0, n / SR, n)
        pad_l = np.zeros(n)
        pad_r = np.zeros(n)
        env = np.sin(math.pi * np.linspace(0.0, 1.0, n)) ** 0.35
        for idx, m in enumerate(chord):
            f = midi_to_hz(m - 12)
            pad_l += np.sin(2 * math.pi * f * 0.998 * st) * 0.055
            pad_r += np.sin(2 * math.pi * f * 1.002 * st) * 0.055
        add_seg(bar * 16 * step_sec, pad_l * env, pad_r * env)

    # 5. Soaring Mega Man X lead synth melody with vibrato!
    for start_step, dur_steps, midi in lead_melody:
        if start_step >= total_steps:
            continue
        f = midi_to_hz(midi)
        n = int(SR * step_sec * dur_steps * 0.96)
        st = np.linspace(0.0, n / SR, n)
        vib = 1.0 + 0.008 * np.sin(2 * math.pi * 5.8 * st) * np.clip(st * 4.0, 0.0, 1.0)
        ph = np.cumsum(2.0 * math.pi * f * vib / SR)
        lead = 0.55 * np.sin(ph) + 0.30 * np.sin(ph * 2.0) + 0.18 * np.sin(ph * 3.0)
        env = (1.0 - np.exp(-st * 160.0)) * np.exp(-st * 2.1)
        sig = lead * env * 0.24
        add_seg(start_step * step_sec, sig * 0.92, sig * 1.08)

    # Subtle stereo delay echo (dotted 8th!) for huge cavernous Cryo server-hall depth
    delay = int(SR * step_sec * 3.0)
    if delay < total_samples:
        left[delay:] += right[:-delay] * 0.24
        right[delay:] += left[:-delay] * 0.24

    return np.stack([left, right], axis=1)


def main() -> None:
    print("Synthesizing Stage 2 Cryo Conduit & Borealis Boss BGM...")
    # Stage 2: 138 BPM D-minor icy heroic synth-rock (8 bars looping cleanly!)
    stage2_bass = [38, 38, 34, 36, 38, 38, 31, 36]  # Dm -> Bb -> C -> Dm -> Gm -> C
    stage2_arps = [
        [62, 65, 69, 74], [62, 65, 69, 72], [58, 62, 65, 70], [60, 64, 67, 72],
        [62, 65, 69, 74], [62, 65, 69, 76], [55, 58, 62, 67], [60, 64, 67, 74],
    ]
    stage2_lead = [
        (0, 3, 74), (3, 3, 77), (6, 2, 76), (8, 4, 74), (12, 4, 72),
        (16, 3, 69), (19, 3, 74), (22, 2, 76), (24, 6, 77), (30, 2, 76),
        (32, 3, 74), (35, 3, 70), (38, 2, 72), (40, 4, 74), (44, 4, 76),
        (48, 3, 79), (51, 3, 77), (54, 2, 76), (56, 8, 72),
        (64, 3, 74), (67, 3, 77), (70, 2, 81), (72, 4, 79), (76, 4, 77),
        (80, 3, 76), (83, 3, 74), (86, 2, 72), (88, 6, 74), (94, 2, 76),
        (96, 3, 79), (99, 3, 77), (102, 2, 74), (104, 4, 70), (108, 4, 74),
        (112, 4, 76), (116, 4, 79), (120, 4, 76), (124, 4, 72),
    ]
    write_ogg("stage2_theme", synth_track(138.0, 8, stage2_bass, stage2_arps, stage2_lead, seed=2085))

    # Boss Borealis: 154 BPM F#-minor relentless crystalline duel (8 bars!)
    boss_bass = [42, 42, 38, 40, 42, 45, 38, 41]  # F#m -> D -> E -> F#m -> A -> D -> C#7
    boss_arps = [
        [66, 69, 73, 78], [66, 69, 73, 76], [62, 66, 69, 74], [64, 68, 71, 76],
        [66, 69, 73, 78], [69, 73, 76, 81], [62, 66, 69, 74], [61, 65, 68, 73],
    ]
    boss_lead = [
        (0, 2, 78), (2, 2, 76), (4, 2, 73), (6, 2, 76), (8, 4, 78), (12, 4, 81),
        (16, 2, 80), (18, 2, 78), (20, 2, 76), (22, 2, 73), (24, 6, 74), (30, 2, 76),
        (32, 3, 74), (35, 3, 78), (38, 2, 76), (40, 4, 73), (44, 4, 71),
        (48, 2, 76), (50, 2, 78), (52, 4, 80), (56, 8, 76),
        (64, 2, 78), (66, 2, 81), (68, 2, 80), (70, 2, 78), (72, 4, 76), (76, 4, 78),
        (80, 3, 81), (83, 3, 83), (86, 2, 81), (88, 4, 78), (92, 4, 76),
        (96, 2, 74), (98, 2, 78), (100, 4, 81), (104, 4, 78), (108, 4, 74),
        (112, 3, 73), (115, 3, 77), (118, 2, 80), (120, 8, 85),
    ]
    write_ogg("boss_borealis", synth_track(154.0, 8, boss_bass, boss_arps, boss_lead, seed=2086))


if __name__ == "__main__":
    main()
