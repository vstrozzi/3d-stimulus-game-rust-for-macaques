#!/usr/bin/env python3
"""
prepare_bevy_textures.py  (v4 — with 1x1 neutral fallback textures)

Usage:
  python prepare_bevy_textures.py <folder>          # single folder
  python prepare_bevy_textures.py <parent_dir>/* # all subfolders (shell glob)
  python prepare_bevy_textures.py * # all subfolders in cwd
"""

import sys
import shutil
from pathlib import Path

try:
    from PIL import Image
    import numpy as np
except ImportError:
    print("Missing dependencies. Install with:")
    print("  pip install Pillow numpy")
    sys.exit(1)

SUFFIXES = {
    "color":        ["_Color"],
    "normal":       ["_NormalGL", "_Normal_GL", "_NormalDX"],
    "roughness":    ["_Roughness"],
    "metalness":    ["_Metalness", "_Metallic"],
    "ao":           ["_AmbientOcclusion", "_AO"],
    "displacement": ["_Displacement", "_Disp"],
}


def find_texture(folder: Path, key: str) -> Path | None:
    for suffix in SUFFIXES[key]:
        for f in folder.iterdir():
            if f.stem.lower().endswith(suffix.lower()) and f.suffix.lower() in (".png", ".jpg", ".jpeg"):
                return f
    return None


def load_linear(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGB"), dtype=np.float32) / 255.0


def load_gray(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("L"), dtype=np.float32) / 255.0


def save_rgb(arr: np.ndarray, out_path: Path, label: str = ""):
    Image.fromarray(np.clip(arr * 255, 0, 255).astype(np.uint8), "RGB").save(out_path)
    print(f"    ✓ {out_path.name}" + (f"  ({label})" if label else ""))


def save_gray(arr: np.ndarray, out_path: Path, label: str = ""):
    Image.fromarray(np.clip(arr * 255, 0, 255).astype(np.uint8), "L").save(out_path)
    print(f"    ✓ {out_path.name}" + (f"  ({label})" if label else ""))


def save_default_rgb(color: tuple[int, int, int], out_path: Path, label: str):
    """Generates a 1x1 pixel texture with a specific fallback color."""
    arr = np.array([[color]], dtype=np.uint8)
    Image.fromarray(arr, "RGB").save(out_path)
    print(f"    + {out_path.name}  (generated fallback: {label})")


def save_default_gray(color: int, out_path: Path, label: str):
    """Generates a 1x1 pixel grayscale texture with a specific fallback value."""
    arr = np.array([[color]], dtype=np.uint8)
    Image.fromarray(arr, "L").save(out_path)
    print(f"    + {out_path.name}  (generated fallback: {label})")


def srgb_to_linear(arr: np.ndarray) -> np.ndarray:
    return np.where(arr <= 0.04045, arr / 12.92, ((arr + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(arr: np.ndarray) -> np.ndarray:
    return np.where(arr <= 0.0031308, arr * 12.92, 1.055 * arr ** (1.0 / 2.4) - 0.055)


def desaturate(arr: np.ndarray, saturation: float = 0.15) -> np.ndarray:
    linear = srgb_to_linear(arr)
    luminance = (
        0.2126 * linear[:, :, 0] +
        0.7152 * linear[:, :, 1] +
        0.0722 * linear[:, :, 2]
    )[:, :, np.newaxis]
    blended = luminance + saturation * (linear - luminance)
    return linear_to_srgb(np.clip(blended, 0, 1))


def prepare(folder: Path) -> bool:
    if not folder.is_dir() or folder.name == "bevy_ready":
        return False

    # 1. Gather all paths first
    color_path = find_texture(folder, "color")
    normal_path = find_texture(folder, "normal")
    rough_path = find_texture(folder, "roughness")
    metal_path = find_texture(folder, "metalness")
    ao_path = find_texture(folder, "ao")
    disp_path = find_texture(folder, "displacement")

    # If NO recognizable maps exist, skip the folder completely
    if not any([color_path, normal_path, rough_path, metal_path, ao_path, disp_path]):
        return False

    out_dir = folder / "bevy_ready"

    # Overwrite: wipe and recreate
    if out_dir.exists():
        shutil.rmtree(out_dir)
        print("    Overwriting existing bevy_ready/")
    out_dir.mkdir()

    print(f"\n  📂 {folder.name}")

    # 1. COLOR
    if color_path:
        arr = load_linear(color_path)
        save_rgb(arr, out_dir / "color.png", "sRGB, natural look")
        desat = desaturate(arr, saturation=0.15)
        save_rgb(desat, out_dir / "color_tintable.png", "near-grayscale, for base_color tinting")
    else:
        save_default_rgb((255, 255, 255), out_dir / "color.png", "white")
        save_default_rgb((255, 255, 255), out_dir / "color_tintable.png", "white")

    # 2. NORMAL
    if normal_path:
        is_dx = any(x in normal_path.stem for x in ["NormalDX", "Normal_DX"])
        arr = load_linear(normal_path)
        if is_dx:
            arr[:, :, 1] = 1.0 - arr[:, :, 1]
            print("    i Converted DX->GL (flipped G channel)")
        save_rgb(arr, out_dir / "normal_gl.png", "linear, is_srgb: false")
    else:
        save_default_rgb((128, 128, 255), out_dir / "normal_gl.png", "flat normal")

    # 3. METALLIC_ROUGHNESS
    if rough_path or metal_path:
        # We need the array dimensions from whichever map actually exists
        reference_path = rough_path if rough_path else metal_path
        h, w = load_gray(reference_path).shape

        roughness = load_gray(rough_path) if rough_path else np.full((h, w), 0.5, dtype=np.float32)
        if not rough_path:
            print("    i No roughness map — padding with 0.5")

        metalness = load_gray(metal_path) if metal_path else np.zeros((h, w), dtype=np.float32)
        if not metal_path:
            print("    i No metalness map — padding with 0.0")

        packed = np.zeros((h, w, 3), dtype=np.float32)
        packed[:, :, 1] = roughness
        packed[:, :, 2] = metalness
        save_rgb(packed, out_dir / "metallic_roughness.png", "linear, G=roughness B=metallic")
    else:
        save_default_rgb((0, 128, 0), out_dir / "metallic_roughness.png", "G=0.5 (rough), B=0 (non-metal)")

    # 4. AMBIENT OCCLUSION
    if ao_path:
        save_gray(load_gray(ao_path), out_dir / "occlusion.png", "linear, is_srgb: false")
    else:
        save_default_gray(255, out_dir / "occlusion.png", "white, fully unoccluded")

    # 5. DISPLACEMENT
    if disp_path:
        save_gray(1.0 - load_gray(disp_path), out_dir / "displacement_inv.png", "inverted for Bevy depth_map")
    else:
        save_default_gray(0, out_dir / "displacement_inv.png", "black, flat depth")

    return True


def resolve_folders(args: list[str]) -> list[Path]:
    folders = []
    seen = set()
    for arg in args:
        p = Path(arg).resolve()
        if p in seen:
            continue
        seen.add(p)
        if p.is_dir():
            folders.append(p)
    return folders


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)

    folders = resolve_folders(sys.argv[1:])

    if not folders:
        print("No valid directories found in the provided arguments.")
        sys.exit(1)

    print(f"\n Processing {len(folders)} folder(s)...\n" + "=" * 48)

    success, skipped = 0, 0
    for folder in folders:
        ok = prepare(folder)
        if ok:
            success += 1
        else:
            skipped += 1

    print(f"\n{'='*48}")
    print(f"  {success} folder(s) processed   {skipped} skipped")
    print(f"{'='*48}")
    print("""
Bevy StandardMaterial fields:

  color.png              -> base_color_texture         (default loader, sRGB)
  color_tintable.png     -> base_color_texture         (+ set base_color to your tint color)
  normal_gl.png          -> normal_map_texture          (is_srgb: false)
  metallic_roughness.png -> metallic_roughness_texture  (is_srgb: false)
                            set metallic = 1.0 and perceptual_roughness = 1.0
  occlusion.png          -> occlusion_texture           (is_srgb: false)
  displacement_inv.png   -> depth_map                   (is_srgb: false)
""")


if __name__ == "__main__":
    main()