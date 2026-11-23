#!/usr/bin/env python3
"""Generates pixel-art Foundry environment textures for *VELOCITY // BREAKER*
using the local WARLOCK ComfyUI GPU server (Flux 2 Klein 4B FP8) + Pillow pixelizer.

Outputs:
  - assets/textures/foundry/foundry_bg_far.png    (640x360 seamless horizontal parallax backdrop)
  - assets/textures/foundry/foundry_bg_mid.png    (640x360 crisp RGBA pixel industrial trusses)
  - assets/textures/foundry/foundry_tile_metal.png (64x64 seamless riveted steel block tile)
"""

from __future__ import annotations

import io
import json
import math
import pathlib
import random
import time
import urllib.parse
import urllib.request
from typing import Any

from PIL import Image, ImageDraw, ImageEnhance

COMFY_URL = "http://192.168.1.151:8188"
OUT_DIR = pathlib.Path("assets/textures/foundry")
PREVIEW_DIR = pathlib.Path("/tmp/foundry_previews")


def build_flux2_workflow(prompt: str, width: int, height: int, seed: int, steps: int = 12) -> dict[str, Any]:
    return {
        "1": {
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": "flux-2-klein-4b-fp8.safetensors",
                "weight_dtype": "fp8_e4m3fn",
            },
        },
        "2": {
            "class_type": "CLIPLoader",
            "inputs": {
                "clip_name": "qwen_3_4b.safetensors",
                "type": "flux2",
            },
        },
        "3": {
            "class_type": "VAELoader",
            "inputs": {
                "vae_name": "flux2-vae.safetensors",
            },
        },
        "6": {
            "class_type": "CLIPTextEncode",
            "inputs": {
                "clip": ["2", 0],
                "text": prompt,
            },
        },
        "8": {
            "class_type": "EmptyFlux2LatentImage",
            "inputs": {
                "width": width,
                "height": height,
                "batch_size": 1,
            },
        },
        "9": {
            "class_type": "RandomNoise",
            "inputs": {
                "noise_seed": seed,
            },
        },
        "10": {
            "class_type": "BasicGuider",
            "inputs": {
                "model": ["1", 0],
                "conditioning": ["6", 0],
            },
        },
        "11": {
            "class_type": "KSamplerSelect",
            "inputs": {
                "sampler_name": "euler",
            },
        },
        "12": {
            "class_type": "BasicScheduler",
            "inputs": {
                "model": ["1", 0],
                "scheduler": "simple",
                "steps": steps,
                "denoise": 1.0,
            },
        },
        "13": {
            "class_type": "SamplerCustomAdvanced",
            "inputs": {
                "noise": ["9", 0],
                "guider": ["10", 0],
                "sampler": ["11", 0],
                "sigmas": ["12", 0],
                "latent_image": ["8", 0],
            },
        },
        "14": {
            "class_type": "VAEDecode",
            "inputs": {
                "samples": ["13", 0],
                "vae": ["3", 0],
            },
        },
        "15": {
            "class_type": "PreviewImage",
            "inputs": {
                "images": ["14", 0],
            },
        },
    }


def render_on_warlock(prompt: str, width: int, height: int, seed: int) -> Image.Image:
    workflow = build_flux2_workflow(prompt, width, height, seed)
    payload = json.dumps({"prompt": workflow}).encode("utf-8")
    req = urllib.request.Request(
        f"{COMFY_URL}/prompt",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=20) as resp:
        prompt_id = json.loads(resp.read().decode("utf-8"))["prompt_id"]
    print(f"[warlock] queued prompt_id={prompt_id} ({width}x{height}, seed={seed})", flush=True)

    t0 = time.time()
    while time.time() - t0 < 240:
        with urllib.request.urlopen(f"{COMFY_URL}/history/{prompt_id}", timeout=15) as resp:
            hist = json.loads(resp.read().decode("utf-8"))
        if prompt_id in hist:
            entry = hist[prompt_id]
            status = entry.get("status", {})
            if status.get("status_str") == "error":
                raise RuntimeError(f"ComfyUI execution error: {status}")
            outputs = entry.get("outputs", {})
            if "15" in outputs and outputs["15"].get("images"):
                img_info = outputs["15"]["images"][0]
                qs = urllib.parse.urlencode(
                    {
                        "filename": img_info["filename"],
                        "subfolder": img_info.get("subfolder", ""),
                        "type": img_info.get("type", "temp"),
                    }
                )
                with urllib.request.urlopen(f"{COMFY_URL}/view?{qs}", timeout=20) as img_resp:
                    raw = img_resp.read()
                print(f"[warlock] completed in {time.time() - t0:.1f}s", flush=True)
                return Image.open(io.BytesIO(raw)).convert("RGB")
        time.sleep(1.5)
    raise TimeoutError(f"Timed out waiting for ComfyUI prompt {prompt_id}")


def make_horizontally_seamless(img: Image.Image, blend_px: int = 48) -> Image.Image:
    """Blends left/right borders via half-shift crossfade so parallax wraps seamlessly."""
    w, h = img.size
    half = w // 2
    shifted = Image.new("RGB", (w, h))
    shifted.paste(img.crop((half, 0, w, h)), (0, 0))
    shifted.paste(img.crop((0, 0, half, h)), (w - half, 0))

    # Build smooth horizontal alpha mask keeping original center & blending seam at center of shifted
    orig_px = img.load()
    shift_px = shifted.load()
    out = shifted.copy()
    out_px = out.load()

    seam_x = w - half
    for x in range(max(0, seam_x - blend_px), min(w, seam_x + blend_px)):
        dist = abs(x - seam_x) / float(blend_px)
        # Cosine ease: 1.0 at seam (use original smooth interior), 0.0 at outer blend edge
        t = 0.5 * (1.0 + math.cos(min(1.0, dist) * math.pi))
        for y in range(h):
            r0, g0, b0 = shift_px[x, y]
            r1, g1, b1 = orig_px[x, y]
            out_px[x, y] = (
                int(r0 * (1.0 - t) + r1 * t),
                int(g0 * (1.0 - t) + g1 * t),
                int(b0 * (1.0 - t) + b1 * t),
            )
    return out


def make_2d_seamless_tile(img: Image.Image, blend_px: int = 12) -> Image.Image:
    """Makes a square texture wrap cleanly in both X and Y."""
    h_wrapped = make_horizontally_seamless(img, blend_px=blend_px)
    rot = h_wrapped.transpose(Image.Transpose.ROTATE_90)
    hv_wrapped = make_horizontally_seamless(rot, blend_px=blend_px)
    return hv_wrapped.transpose(Image.Transpose.ROTATE_270)


def pixelize_foundry_backdrop(raw: Image.Image) -> Image.Image:
    """Downsamples to crisp 320x180 pixel grid -> 640x360 2x pixels, darkens for readability."""
    raw = raw.resize((640, 360), Image.Resampling.LANCZOS)
    seamless = make_horizontally_seamless(raw, blend_px=56)
    # Darken slightly and boost deep midnight blue/amber contrast so bright sprites read cleanly
    darkened = ImageEnhance.Brightness(seamless).enhance(0.46)
    contrasted = ImageEnhance.Contrast(darkened).enhance(1.18)
    # Crisp 2px pixel-art quantization (320x180 -> 32-color palette -> 640x360 nearest)
    low = contrasted.resize((320, 180), Image.Resampling.BILINEAR)
    quant = low.quantize(colors=32, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGB")
    return quant.resize((640, 360), Image.Resampling.NEAREST)


def pixelize_metal_tile(raw: Image.Image) -> Image.Image:
    """Turns a rendered steel panel into a crisp 64x64 seamless industrial pixel tile."""
    # Center-crop square
    w, h = raw.size
    side = min(w, h)
    left = (w - side) // 2
    top = (h - side) // 2
    sq = raw.crop((left, top, left + side, top + side))
    sq = sq.resize((128, 128), Image.Resampling.LANCZOS)
    seamless = make_2d_seamless_tile(sq, blend_px=20)
    # Keep dark cool steel range so cyan/amber neon edge rails glow cleanly on top
    dark = ImageEnhance.Brightness(seamless).enhance(0.58)
    crisp = ImageEnhance.Contrast(dark).enhance(1.22)
    low = crisp.resize((64, 64), Image.Resampling.BILINEAR)
    quant = low.quantize(colors=20, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE).convert("RGB")

    # Add crisp 1px dark bevel seam & corner rivets on a 32x32 sub-grid for iconic Mega Man / Foundry feel
    draw = ImageDraw.Draw(quant)
    seam_dark = (18, 24, 36)
    highlight = (68, 86, 114)
    rivet = (86, 108, 140)
    for ox in (0, 32):
        for oy in (0, 32):
            draw.rectangle([ox, oy, ox + 31, oy + 31], outline=seam_dark)
            draw.line([ox + 1, oy + 1, ox + 30, oy + 1], fill=highlight)
            draw.line([ox + 1, oy + 1, ox + 1, oy + 30], fill=highlight)
            for rx, ry in [(ox + 4, oy + 4), (ox + 26, oy + 4), (ox + 4, oy + 26), (ox + 26, oy + 26)]:
                draw.rectangle([rx, ry, rx + 1, ry + 1], fill=rivet)
    return quant


def build_midground_parallax_trusses() -> Image.Image:
    """Builds a crisp 640x360 RGBA transparent pixel-art mid-ground industrial silhouette layer
    (steel pillars, diagonal gantry trusses, hanging smelter chains, subtle amber vent slits)."""
    img = Image.new("RGBA", (640, 360), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    steel_dark = (14, 19, 30, 215)
    steel_mid = (24, 34, 50, 225)
    steel_rim = (42, 60, 86, 235)
    vent_amber = (195, 98, 28, 210)
    vent_cyan = (28, 150, 185, 200)

    # Vertical heavy foundry columns spaced cleanly across 640px loop
    for cx in (48, 224, 400, 560):
        d.rectangle([cx, 0, cx + 24, 360], fill=steel_dark)
        d.rectangle([cx + 2, 0, cx + 5, 360], fill=steel_mid)
        d.line([cx, 0, cx, 360], fill=steel_rim)
        # Cross bracing notches inside pillar
        for y in range(16, 360, 32):
            d.rectangle([cx + 6, y, cx + 18, y + 6], fill=steel_mid)
            if y % 64 == 16:
                d.rectangle([cx + 9, y + 2, cx + 15, y + 4], fill=vent_amber)

    # Upper horizontal crane gantry girder with crisp X-truss
    gy = 44
    d.rectangle([0, gy, 640, gy + 16], fill=steel_dark)
    d.line([0, gy, 640, gy], fill=steel_rim)
    d.line([0, gy + 16, 640, gy + 16], fill=steel_mid)
    for x in range(0, 640, 32):
        d.line([x, gy + 1, x + 31, gy + 15], fill=steel_mid, width=2)
        d.line([x + 31, gy + 1, x, gy + 15], fill=steel_mid, width=2)

    # Mid-height catwalk rail
    cy = 208
    d.rectangle([0, cy, 640, cy + 8], fill=steel_dark)
    d.line([0, cy, 640, cy], fill=steel_rim)
    d.line([0, cy - 10, 640, cy - 10], fill=steel_mid)
    for x in range(0, 640, 24):
        d.line([x, cy - 10, x, cy], fill=steel_mid)

    # Hanging smelter chains & distant coolant pipes
    for hx, length in [(136, 130), (312, 95), (484, 145)]:
        for y in range(gy + 16, gy + 16 + length, 8):
            d.rectangle([hx, y, hx + 3, y + 5], outline=steel_mid)
        # Small glowing foundry conduit beacon at pipe end
        d.rectangle([hx - 3, gy + 16 + length, hx + 6, gy + 24 + length], fill=steel_dark)
        d.rectangle([hx - 1, gy + 18 + length, hx + 4, gy + 22 + length], fill=vent_cyan)

    # Lower furnace bay silhouette band with glowing vents
    d.rectangle([0, 316, 640, 360], fill=steel_dark)
    d.line([0, 316, 640, 316], fill=steel_rim)
    for vx in range(24, 640, 80):
        d.rectangle([vx, 326, vx + 36, 344], fill=(10, 12, 18, 240))
        for sy in (330, 336, 341):
            d.line([vx + 4, sy, vx + 32, sy], fill=vent_amber)

    return img


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    PREVIEW_DIR.mkdir(parents=True, exist_ok=True)

    bg_prompt = (
        "16-bit retro pixel art video game side-scrolling background of a colossal futuristic "
        "cyberpunk steel foundry interior cavern, towering dark gunmetal girders and smelting pipes, "
        "distant glowing molten amber furnace vents and cool electric cyan plasma conduits in deep "
        "midnight slate shadows, clean side-view orthographic parallax backdrop, crisp pixel clusters, "
        "no characters, no text, no UI, Mega Man X and Sonic CD inspired industrial stage atmosphere"
    )
    tile_prompt = (
        "seamless 16-bit retro pixel art industrial dark gunmetal steel metal wall panel texture, "
        "brushed dark blue-grey forged iron plates with subtle bolts, recessed seams, crisp flat "
        "top-down orthographic game tileset texture, high contrast metallic bevels, no perspective, "
        "no text, no characters"
    )

    seed_bg = 42077
    seed_tile = 91823

    print("[foundry_art] Generating parallax backdrop on WARLOCK RTX 3060...", flush=True)
    raw_bg = render_on_warlock(bg_prompt, 1024, 576, seed_bg)
    raw_bg.save(PREVIEW_DIR / "raw_bg.jpg", quality=85)

    pixel_bg = pixelize_foundry_backdrop(raw_bg)
    bg_path = OUT_DIR / "foundry_bg_far.png"
    pixel_bg.save(bg_path)
    print(f"[foundry_art] Wrote {bg_path} ({pixel_bg.size})", flush=True)

    print("[foundry_art] Generating riveted metal tile texture on WARLOCK RTX 3060...", flush=True)
    raw_tile = render_on_warlock(tile_prompt, 768, 768, seed_tile)
    raw_tile.save(PREVIEW_DIR / "raw_tile.jpg", quality=85)

    pixel_tile = pixelize_metal_tile(raw_tile)
    tile_path = OUT_DIR / "foundry_tile_metal.png"
    pixel_tile.save(tile_path)
    print(f"[foundry_art] Wrote {tile_path} ({pixel_tile.size})", flush=True)

    mid_img = build_midground_parallax_trusses()
    mid_path = OUT_DIR / "foundry_bg_mid.png"
    mid_img.save(mid_path)
    print(f"[foundry_art] Wrote {mid_path} ({mid_img.size})", flush=True)


if __name__ == "__main__":
    main()
