# VELOCITY // BREAKER — Soundtrack Files (`assets/audio/music/`)

The looping BGM engine (`src/audio/music.rs`) automatically detects `.ogg` (Vorbis, preferred) or `.mp3` files in this folder and smoothly crossfades over `0.8s` between Stage and Boss encounters. If any track is missing, the game logs a single `INFO` notice and continues silently without panicking.

## Expected Filenames

| Filename | Usage | Playback Mode |
|---|---|---|
| `title_theme.ogg` | Title / Main Menu | Loops continuously |
| `stage1_theme.ogg` | Stage 1: Foundry Citadel (Acts 1–4) | Loops continuously, ducks gently on Pause |
| `boss_theme.ogg` | Boss Sanctum (*IRON TITAN: VULCANUS*) | Crossfades in over `0.8s` on Boss lockdown, fades out on KO |
| `stage_clear.ogg` | Stage 1 Clear Victory Screen | Plays once triumphantly |

## Converting `.wav` or `.mp3` to `.ogg` (Vorbis)

```bash
ffmpeg -y -i input.wav -c:a libvorbis -q:a 5 assets/audio/music/stage1_theme.ogg
```
