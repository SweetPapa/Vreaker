#!/usr/bin/env python3
"""Generates high-quality textured 3D character `.glb` meshes from clean front-view A-pose
concept art using the local WARLOCK ComfyUI GPU server's native TRELLIS.2 pipeline
(BiRefNet -> DINOv3 -> Structure -> Shape 512 -> Upsample 1024 -> RemeshMesh -> DecimateMesh
-> UnwrapMesh -> BakeTextureFromVoxel + BakeNormalMapFromMesh -> ApplyTextureToMesh -> SaveGLB).

Usage:
    python3 tools/generate_character_3d.py \
        --input raw_assets/concepts/levy_apose_v1.png \
        --out raw_assets/characters/levy_raw.glb \
        --name levy
"""

from __future__ import annotations

import argparse
import io
import json
import pathlib
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
from typing import Any

from PIL import Image

DEFAULT_COMFY_URL = "http://192.168.1.151:8188"


def pad_image_to_square_png(src: pathlib.Path, pad_ratio: float = 0.08) -> bytes:
    """Pads an A-pose image symmetrically with its corner background color so TRELLIS.2
    and BiRefNet never clip head/boots/hands."""
    with Image.open(src) as raw:
        rgb = raw.convert("RGB")
        w, h = rgb.size
        bg = rgb.getpixel((4, 4))
        pad_x = int(w * pad_ratio)
        pad_y = int(h * pad_ratio)
        cw = w + pad_x * 2
        ch = h + pad_y * 2
        side = max(cw, ch)
        canvas = Image.new("RGB", (side, side), bg)
        ox = (side - w) // 2
        oy = (side - h) // 2
        canvas.paste(rgb, (ox, oy))
        if side > 1280:
            canvas = canvas.resize((1280, 1280), Image.Resampling.LANCZOS)
        buf = io.BytesIO()
        canvas.save(buf, format="PNG")
        return buf.getvalue()


def upload_image_to_comfy(comfy_url: str, filename: str, png_bytes: bytes) -> str:
    """Uploads a PNG to ComfyUI's `/upload/image` endpoint using multipart/form-data."""
    boundary = f"----VreakerBoundary{uuid.uuid4().hex}"
    body = bytearray()
    body.extend(f"--{boundary}\r\n".encode("utf-8"))
    body.extend(
        f'Content-Disposition: form-data; name="image"; filename="{filename}"\r\n'.encode("utf-8")
    )
    body.extend(b"Content-Type: image/png\r\n\r\n")
    body.extend(png_bytes)
    body.extend(b"\r\n")
    body.extend(f"--{boundary}\r\n".encode("utf-8"))
    body.extend(b'Content-Disposition: form-data; name="overwrite"\r\n\r\ntrue\r\n')
    body.extend(f"--{boundary}--\r\n".encode("utf-8"))

    req = urllib.request.Request(
        f"{comfy_url.rstrip('/')}/upload/image",
        data=bytes(body),
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    return data["name"]


def build_trellis2_workflow(
    uploaded_image: str,
    prefix: str,
    seed: int = 42,
    remesh_res: int = 384,
    target_faces: int = 75000,
    texture_res: int = 2048,
    upsample_res: int = 1024,
    structure_steps: int = 12,
    shape_steps: int = 20,
    texture_steps: int = 12,
) -> dict[str, Any]:
    """Constructs the official native ComfyUI TRELLIS.2 high-quality Image-to-GLB workflow."""
    wf: dict[str, Any] = {
        "1": {
            "class_type": "LoadImage",
            "inputs": {"image": uploaded_image},
        },
        "2": {
            "class_type": "LoadBackgroundRemovalModel",
            "inputs": {"bg_removal_name": "jh\\birefnet.safetensors"},
        },
        "3": {
            "class_type": "RemoveBackground",
            "inputs": {
                "bg_removal_model": ["2", 0],
                "image": ["1", 0],
            },
        },
        "4": {
            "class_type": "ImageCropToMask",
            "inputs": {
                "images": ["1", 0],
                "masks": ["3", 0],
                "width": 1024,
                "height": 1024,
                "pad_factor": 1.08,
                "grow_mask": 0,
                "background": "#000000",
            },
        },
        "5": {
            "class_type": "CLIPVisionLoader",
            "inputs": {"clip_name": "jh\\dino_v3_vit_l.safetensors"},
        },
        "6": {
            "class_type": "Trellis2Conditioning",
            "inputs": {
                "clip_vision_model": ["5", 0],
                "image": ["4", 0],
            },
        },
        "7": {
            "class_type": "UNETLoader",
            "inputs": {
                "unet_name": "jh\\trellis_2_int8_convrot.safetensors",
                "weight_dtype": "default",
            },
        },
        "8": {
            "class_type": "VAELoader",
            "inputs": {"vae_name": "jh\\trellis_2_shape_vae_bf16.safetensors"},
        },
        "9": {
            "class_type": "VAELoader",
            "inputs": {"vae_name": "jh\\trellis_2_texture_vae_bf16.safetensors"},
        },
        # --- Stage 1: Dense Structure (32^3) ---
        "10": {
            "class_type": "CFGOverride",
            "inputs": {
                "model": ["7", 0],
                "cfg": 1.0,
                "start_percent": 0.667,
                "end_percent": 1.0,
            },
        },
        "11": {
            "class_type": "RescaleCFG",
            "inputs": {
                "model": ["10", 0],
                "multiplier": 0.7,
            },
        },
        "12": {
            "class_type": "ModelSamplingSD3",
            "inputs": {
                "model": ["11", 0],
                "shift": 5.0,
            },
        },
        "13": {
            "class_type": "EmptyTrellis2LatentStructure",
            "inputs": {"batch_size": 1},
        },
        "14": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["12", 0],
                "seed": seed,
                "steps": structure_steps,
                "cfg": 7.5,
                "sampler_name": "euler",
                "scheduler": "normal",
                "positive": ["6", 0],
                "negative": ["6", 1],
                "latent_image": ["13", 0],
                "denoise": 1.0,
            },
        },
        "15": {
            "class_type": "VaeDecodeStructureTrellis2",
            "inputs": {
                "samples": ["14", 0],
                "vae": ["8", 0],
                "resolution": "32",
            },
        },
        # --- Stage 2: Sparse Shape (512) ---
        "16": {
            "class_type": "Trellis2ShapeStage",
            "inputs": {
                "positive": ["6", 0],
                "negative": ["6", 1],
                "voxel": ["15", 0],
            },
        },
        "17": {
            "class_type": "CFGOverride",
            "inputs": {
                "model": ["7", 0],
                "cfg": 1.0,
                "start_percent": 0.769,
                "end_percent": 1.0,
            },
        },
        "18": {
            "class_type": "RescaleCFG",
            "inputs": {
                "model": ["17", 0],
                "multiplier": 0.5,
            },
        },
        "19": {
            "class_type": "KSampler",
            "inputs": {
                "model": ["18", 0],
                "seed": seed,
                "steps": shape_steps,
                "cfg": 7.5,
                "sampler_name": "euler",
                "scheduler": "normal",
                "positive": ["16", 0],
                "negative": ["16", 1],
                "latent_image": ["16", 2],
                "denoise": 1.0,
            },
        },
    }

    if upsample_res >= 1024:
        wf["20"] = {
            "class_type": "Trellis2UpsampleStage",
            "inputs": {
                "positive": ["16", 0],
                "negative": ["16", 1],
                "shape_latent": ["19", 0],
                "vae": ["8", 0],
                "target_resolution": upsample_res,
            },
        }
        wf["21"] = {
            "class_type": "KSampler",
            "inputs": {
                "model": ["18", 0],
                "seed": seed,
                "steps": 12,
                "cfg": 7.5,
                "sampler_name": "euler",
                "scheduler": "simple",
                "positive": ["20", 0],
                "negative": ["20", 1],
                "latent_image": ["20", 2],
                "denoise": 1.0,
            },
        }
        shape_latent_ref = ["21", 0]
        tex_pos_ref = ["20", 0]
        tex_neg_ref = ["20", 1]
    else:
        shape_latent_ref = ["19", 0]
        tex_pos_ref = ["16", 0]
        tex_neg_ref = ["16", 1]

    # --- Stage 3: Shape Decode + Texture Diffusion + Texture Decode ---
    wf["22"] = {
        "class_type": "VaeDecodeShapeTrellis",
        "inputs": {
            "samples": shape_latent_ref,
            "vae": ["8", 0],
        },
    }
    wf["23"] = {
        "class_type": "Trellis2TextureStage",
        "inputs": {
            "positive": tex_pos_ref,
            "negative": tex_neg_ref,
            "shape_latent": shape_latent_ref,
        },
    }
    wf["24"] = {
        "class_type": "KSampler",
        "inputs": {
            "model": ["7", 0],
            "seed": seed + 1,
            "steps": texture_steps,
            "cfg": 1.0,
            "sampler_name": "euler",
            "scheduler": "normal",
            "positive": ["23", 0],
            "negative": ["23", 1],
            "latent_image": ["23", 2],
            "denoise": 1.0,
        },
    }
    wf["25"] = {
        "class_type": "VaeDecodeTextureTrellis",
        "inputs": {
            "samples": ["24", 0],
            "vae": ["9", 0],
            "shape_subdivides": ["22", 1],
        },
    }

    # --- Stage 4: Clean Manifold Remesh -> Smooth Decimate -> UV Unwrap -> PBR + Normal Bake ---
    wf["26"] = {
        "class_type": "RemeshMesh",
        "inputs": {
            "mesh": ["22", 0],
            "resolution": remesh_res,
            "sign_mode": "udf",
            "sign_mode.qef": False,
            "sign_mode.drop_inverted_components": True,
            "sign_mode.drop_enclosed_components": True,
            "band": 1.0,
            "project_back": 0.4,
            "fix_poles": True,
            "smooth_iters": 4,
            "drop_small_components": 0.01,
            "precluster_max_verts": 20000000,
        },
    }
    wf["27"] = {
        "class_type": "DecimateMesh",
        "inputs": {
            "mesh": ["26", 0],
            "target_face_count": target_faces,
            "placement_mode": "midpoint",
        },
    }
    wf["28"] = {
        "class_type": "MeshSmoothNormals",
        "inputs": {
            "mesh": ["27", 0],
            "crease_angle": 180.0,
        },
    }
    wf["29"] = {
        "class_type": "UnwrapMesh",
        "inputs": {
            "mesh": ["28", 0],
            "segmenter": "pec",
            "resolution": texture_res,
            "padding": 2,
            "weld_distance": 0.0002,
        },
    }
    wf["30"] = {
        "class_type": "BakeTextureFromVoxel",
        "inputs": {
            "mesh": ["29", 0],
            "voxel_colors": ["25", 0],
            "texture_size": texture_res,
            "reference_mesh": ["22", 0],
        },
    }
    wf["31"] = {
        "class_type": "BakeNormalMapFromMesh",
        "inputs": {
            "low_poly": ["29", 0],
            "high_poly": ["26", 0],
            "resolution": 1024,
            "cage_distance": 0.05,
            "ignore_backfaces": True,
        },
    }
    wf["32"] = {
        "class_type": "ApplyTextureToMesh",
        "inputs": {
            "mesh": ["29", 0],
            "base_color": ["30", 0],
            "metallic": ["30", 1],
            "roughness": ["30", 2],
            "normal_map": ["31", 0],
        },
    }
    wf["33"] = {
        "class_type": "MeshSmoothNormals",
        "inputs": {
            "mesh": ["32", 0],
            "crease_angle": 180.0,
        },
    }
    wf["34"] = {
        "class_type": "SaveGLB",
        "inputs": {
            "mesh": ["33", 0],
            "filename_prefix": f"3d/{prefix}",
        },
    }
    return wf


def queue_and_download_glb(
    comfy_url: str,
    workflow: dict[str, Any],
    out_path: pathlib.Path,
    timeout_secs: int = 720,
) -> pathlib.Path:
    base = comfy_url.rstrip("/")
    payload = json.dumps({"prompt": workflow}).encode("utf-8")
    req = urllib.request.Request(
        f"{base}/prompt",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            res = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        err_body = e.read().decode("utf-8", errors="replace")
        raise RuntimeError(f"ComfyUI rejected prompt ({e.code}): {err_body}") from e

    prompt_id = res["prompt_id"]
    print(f"[trellis2] queued prompt_id={prompt_id}", flush=True)

    t0 = time.time()
    while time.time() - t0 < timeout_secs:
        with urllib.request.urlopen(f"{base}/history/{prompt_id}", timeout=20) as resp:
            hist = json.loads(resp.read().decode("utf-8"))
        if prompt_id in hist:
            entry = hist[prompt_id]
            status = entry.get("status", {})
            if status.get("status_str") == "error":
                raise RuntimeError(f"TRELLIS.2 execution error: {json.dumps(status, indent=2)}")
            outputs = entry.get("outputs", {})
            print(f"[trellis2] finished in {time.time() - t0:.1f}s, outputs={list(outputs.keys())}", flush=True)
            file_info = None
            for _, node_out in outputs.items():
                for key in ("result", "3d", "meshes", "files", "images", "gltf"):
                    items = node_out.get(key)
                    if isinstance(items, list) and items:
                        first = items[0]
                        if isinstance(first, dict) and "filename" in first:
                            file_info = first
                            break
                        elif isinstance(first, str) and first.lower().endswith((".glb", ".gltf")):
                            p = pathlib.PurePosixPath(first.replace("\\", "/"))
                            file_info = {
                                "filename": p.name,
                                "subfolder": str(p.parent) if str(p.parent) != "." else "",
                                "type": "output",
                            }
                            break
                if file_info:
                    break
            if not file_info:
                raise RuntimeError(f"Could not locate saved GLB in outputs: {json.dumps(outputs, indent=2)}")

            qs = urllib.parse.urlencode(
                {
                    "filename": file_info["filename"],
                    "subfolder": file_info.get("subfolder", ""),
                    "type": file_info.get("type", "output"),
                }
            )
            print(f"[trellis2] downloading {file_info}...", flush=True)
            with urllib.request.urlopen(f"{base}/view?{qs}", timeout=60) as dl:
                glb_bytes = dl.read()
            out_path.parent.mkdir(parents=True, exist_ok=True)
            out_path.write_bytes(glb_bytes)
            print(f"[trellis2] wrote {out_path} ({len(glb_bytes):,} bytes)", flush=True)
            return out_path
        elapsed = time.time() - t0
        if int(elapsed) % 20 == 0 and elapsed >= 18:
            print(f"[trellis2] still running ({elapsed:.0f}s)...", flush=True)
        time.sleep(3.0)
    raise TimeoutError(f"Timed out waiting for TRELLIS.2 prompt {prompt_id}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate a sculpted 3D GLB mesh on WARLOCK TRELLIS.2.")
    parser.add_argument("--input", required=True, type=pathlib.Path, help="Input front-view A-pose image.")
    parser.add_argument("--out", required=True, type=pathlib.Path, help="Output .glb path.")
    parser.add_argument("--name", default="character", help="Filename prefix on WARLOCK.")
    parser.add_argument("--comfy-url", default=DEFAULT_COMFY_URL, help="WARLOCK ComfyUI URL.")
    parser.add_argument("--seed", type=int, default=42, help="Diffusion seed.")
    parser.add_argument("--remesh", type=int, default=384, help="RemeshMesh voxel grid resolution (default: 384).")
    parser.add_argument("--faces", type=int, default=75000, help="Target decimated face count (default: 75000).")
    parser.add_argument("--tex-res", type=int, default=2048, help="Baked texture atlas size (default: 2048).")
    parser.add_argument("--upsample", type=int, default=1024, help="Upsample shape res (0=512 fast, 1024=high).")
    args = parser.parse_args()

    png_bytes = pad_image_to_square_png(args.input)
    uploaded = upload_image_to_comfy(args.comfy_url, f"vbreaker_{args.name}_apose.png", png_bytes)
    print(f"[trellis2] uploaded {args.input} -> {uploaded}", flush=True)

    wf = build_trellis2_workflow(
        uploaded_image=uploaded,
        prefix=f"vbreaker_{args.name}",
        seed=args.seed,
        remesh_res=args.remesh,
        target_faces=args.faces,
        texture_res=args.tex_res,
        upsample_res=args.upsample,
    )
    queue_and_download_glb(args.comfy_url, wf, args.out)


if __name__ == "__main__":
    main()
