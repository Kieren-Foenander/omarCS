from __future__ import annotations

import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any

import numpy as np
import trimesh
from trimesh.ray.ray_pyembree import RayMeshIntersector

from .config import data_home

FOV_COSINE = 0.5735764363510462


def vrf_binary() -> Path:
    return data_home() / "omarcs/vrf/Source2Viewer-CLI"


def cs2_maps_root() -> Path | None:
    candidates = (
        data_home()
        / "Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/maps",
        Path.home()
        / ".steam/steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/maps",
    )
    return next((path for path in candidates if path.exists()), None)


def geometry_root() -> Path:
    return data_home() / "omarcs/geometry"


def geometry_path(map_name: str) -> Path | None:
    maps = cs2_maps_root()
    binary = vrf_binary()
    if (
        maps is None
        or not binary.exists()
        or not map_name.startswith(("de_", "cs_", "ar_"))
    ):
        return None
    vpk = maps / f"{map_name}.vpk"
    if not vpk.exists():
        return None

    root = geometry_root()
    root.mkdir(parents=True, exist_ok=True)
    output = root / f"{map_name}-physics.glb"
    metadata = root / f"{map_name}.json"
    signature = {"size": vpk.stat().st_size, "mtimeNs": vpk.stat().st_mtime_ns}
    try:
        if (
            output.exists()
            and json.loads(metadata.read_text(encoding="utf-8")) == signature
        ):
            return output
    except (OSError, json.JSONDecodeError):
        pass

    with tempfile.TemporaryDirectory(prefix=f"omarcs-{map_name}-") as temporary:
        command = [
            str(binary),
            "-i",
            str(vpk),
            "-o",
            temporary,
            "-f",
            f"maps/{map_name}/world_physics.vmdl_c",
            "-d",
            "--gltf_export_format",
            "glb",
        ]
        result = subprocess.run(
            command,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
            timeout=120,
            check=False,
        )
        generated = next(Path(temporary).rglob("world_physics_physics.glb"), None)
        if result.returncode or generated is None:
            return None
        handle, temporary_name = tempfile.mkstemp(
            prefix=f"{map_name}.", suffix=".glb", dir=root
        )
        os.close(handle)
        try:
            shutil.copyfile(generated, temporary_name)
            os.replace(temporary_name, output)
        finally:
            Path(temporary_name).unlink(missing_ok=True)
        metadata.write_text(json.dumps(signature) + "\n", encoding="utf-8")
    return output


def load_map_mesh(map_name: str) -> RayMeshIntersector | None:
    try:
        path = geometry_path(map_name)
        if path is None:
            return None
        loaded = trimesh.load(path)
        if not isinstance(loaded, trimesh.Scene):
            return RayMeshIntersector(loaded)
        non_occluding = (
            "blocksound",
            "chainlink",
            "glass",
            "grenadeclip",
            "ladder",
            "metalgrate",
            "npcclip",
            "passbullets",
            "playerclip",
            "sky",
        )
        geometry = [
            mesh
            for name, mesh in loaded.geometry.items()
            if not any(token in name.casefold() for token in non_occluding)
        ]
        return (
            RayMeshIntersector(trimesh.util.concatenate(geometry)) if geometry else None
        )
    except (OSError, RuntimeError, ValueError, subprocess.SubprocessError):
        return None


def visible_rows(
    rows: list[dict[str, Any]], intersector: RayMeshIntersector
) -> list[bool]:
    if not rows:
        return []
    origins: list[list[float]] = []
    targets: list[list[float]] = []
    owners: list[int] = []
    for index, row in enumerate(rows):
        viewer_duck = float(row.get("viewer_duck") or 0.0)
        target_duck = float(row.get("target_duck") or 0.0)
        origin = np.array(
            [
                float(row["viewer_x"]),
                float(row["viewer_y"]),
                float(row["viewer_z"]) + 64 - 18 * viewer_duck,
            ]
        )
        pitch = np.radians(float(row["viewer_pitch"]))
        yaw = np.radians(float(row["viewer_yaw"]))
        view = np.array(
            [np.cos(pitch) * np.cos(yaw), np.cos(pitch) * np.sin(yaw), -np.sin(pitch)]
        )
        for height in (
            64 - 18 * target_duck,
            50 - 12 * target_duck,
            38 - 8 * target_duck,
        ):
            target = np.array(
                [
                    float(row["target_x"]),
                    float(row["target_y"]),
                    float(row["target_z"]) + height,
                ]
            )
            direction = target - origin
            distance = np.linalg.norm(direction)
            if distance <= 0:
                continue
            unit = direction / distance
            if float(np.dot(view, unit)) < FOV_COSINE:
                continue
            origins.append(origin.tolist())
            targets.append(target.tolist())
            owners.append(index)

    visible = [False] * len(rows)
    if not origins:
        return visible
    origin_array = np.asarray(origins)
    target_array = np.asarray(targets)
    vectors = target_array - origin_array
    distances = np.linalg.norm(vectors, axis=1)
    directions = vectors / distances[:, None]
    blocked_at: dict[int, float] = {}
    batch_size = 32
    for start in range(0, len(origin_array), batch_size):
        stop = start + batch_size
        locations, ray_indices, _ = intersector.intersects_location(
            origin_array[start:stop], directions[start:stop], multiple_hits=False
        )
        for location, local_ray in zip(locations, ray_indices):
            ray = start + int(local_ray)
            blocked_at[ray] = float(np.linalg.norm(location - origin_array[ray]))
    for ray, owner in enumerate(owners):
        collision = blocked_at.get(ray)
        if collision is None or collision >= distances[ray] - 2.0:
            visible[owner] = True
    return visible
