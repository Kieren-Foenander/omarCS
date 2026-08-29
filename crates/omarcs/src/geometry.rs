use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

const FOV_COSINE: f32 = 0.573_576_436_351_046_2;
const HIT_TOLERANCE: f32 = 2.0;
const TARGET_HEIGHTS: [fn(f32) -> f32; 3] = [
    |duck| 64.0 - 18.0 * duck,
    |duck| 50.0 - 12.0 * duck,
    |duck| 38.0 - 8.0 * duck,
];
const NON_OCCLUDING: &[&str] = &[
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
];

#[derive(Clone, Copy, Debug)]
pub struct VisibilityRow {
    pub viewer_origin: [f32; 3],
    pub viewer_duck: f32,
    pub viewer_pitch: f32,
    pub viewer_yaw: f32,
    pub target_origin: [f32; 3],
    pub target_duck: f32,
}

#[derive(Debug)]
pub struct Mesh {
    triangles: Vec<Triangle>,
    nodes: Vec<BvhNode>,
    root: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    a: [f32; 3],
    b: [f32; 3],
    c: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
struct Aabb {
    min: [f32; 3],
    max: [f32; 3],
}

#[derive(Clone, Copy, Debug)]
struct BvhNode {
    bounds: Aabb,
    left: u32,
    right: u32,
}

impl Mesh {
    pub fn from_triangles(faces: impl IntoIterator<Item = [[f32; 3]; 3]>) -> Self {
        let triangles = faces
            .into_iter()
            .map(|[a, b, c]| Triangle { a, b, c })
            .collect::<Vec<_>>();
        let (nodes, root) = build_bvh(&triangles);
        Self {
            triangles,
            nodes,
            root,
        }
    }

    #[cfg(test)]
    pub fn axis_aligned_box(extents: [f32; 3], translation: [f32; 3]) -> Self {
        let half = [extents[0] * 0.5, extents[1] * 0.5, extents[2] * 0.5];
        let min = [
            translation[0] - half[0],
            translation[1] - half[1],
            translation[2] - half[2],
        ];
        let max = [
            translation[0] + half[0],
            translation[1] + half[1],
            translation[2] + half[2],
        ];
        let p = |x, y, z| [x, y, z];
        Self::from_triangles([
            [
                p(min[0], min[1], min[2]),
                p(min[0], max[1], min[2]),
                p(min[0], max[1], max[2]),
            ],
            [
                p(min[0], min[1], min[2]),
                p(min[0], max[1], max[2]),
                p(min[0], min[1], max[2]),
            ],
            [
                p(max[0], min[1], min[2]),
                p(max[0], min[1], max[2]),
                p(max[0], max[1], max[2]),
            ],
            [
                p(max[0], min[1], min[2]),
                p(max[0], max[1], max[2]),
                p(max[0], max[1], min[2]),
            ],
            [
                p(min[0], min[1], min[2]),
                p(max[0], min[1], min[2]),
                p(max[0], min[1], max[2]),
            ],
            [
                p(min[0], min[1], min[2]),
                p(max[0], min[1], max[2]),
                p(min[0], min[1], max[2]),
            ],
            [
                p(min[0], max[1], min[2]),
                p(min[0], max[1], max[2]),
                p(max[0], max[1], max[2]),
            ],
            [
                p(min[0], max[1], min[2]),
                p(max[0], max[1], max[2]),
                p(max[0], max[1], min[2]),
            ],
            [
                p(min[0], min[1], min[2]),
                p(min[0], max[1], min[2]),
                p(max[0], max[1], min[2]),
            ],
            [
                p(min[0], min[1], min[2]),
                p(max[0], max[1], min[2]),
                p(max[0], min[1], min[2]),
            ],
            [
                p(min[0], min[1], max[2]),
                p(max[0], min[1], max[2]),
                p(max[0], max[1], max[2]),
            ],
            [
                p(min[0], min[1], max[2]),
                p(max[0], max[1], max[2]),
                p(min[0], max[1], max[2]),
            ],
        ])
    }

    fn closest_hit(&self, origin: [f32; 3], direction: [f32; 3], max_t: f32) -> Option<f32> {
        let root = self.root?;
        let mut stack = vec![root];
        let mut closest = None;
        while let Some(index) = stack.pop() {
            let node = self.nodes[index as usize];
            let limit = closest.unwrap_or(max_t);
            if !ray_hits_aabb(origin, direction, node.bounds, limit) {
                continue;
            }
            if node.right == u32::MAX {
                if let Some(distance) =
                    triangle_hit(origin, direction, self.triangles[node.left as usize], limit)
                {
                    closest = Some(distance);
                }
            } else {
                stack.push(node.left);
                stack.push(node.right);
            }
        }
        closest
    }
}

pub fn visible_rows(rows: &[VisibilityRow], mesh: &Mesh) -> Vec<bool> {
    let mut visible = vec![false; rows.len()];
    for (index, row) in rows.iter().enumerate() {
        let origin = eye(row.viewer_origin, row.viewer_duck);
        let view = view_direction(row.viewer_pitch, row.viewer_yaw);
        for height in TARGET_HEIGHTS {
            let target = [
                row.target_origin[0],
                row.target_origin[1],
                row.target_origin[2] + height(row.target_duck),
            ];
            let offset = sub(target, origin);
            let distance = length(offset);
            if distance <= 0.0 {
                continue;
            }
            let direction = [
                offset[0] / distance,
                offset[1] / distance,
                offset[2] / distance,
            ];
            if dot(view, direction) < FOV_COSINE {
                continue;
            }
            let blocked = mesh
                .closest_hit(origin, direction, distance)
                .is_some_and(|hit| hit < distance - HIT_TOLERANCE);
            if !blocked {
                visible[index] = true;
                break;
            }
        }
    }
    visible
}

pub fn load_map_mesh(map_name: &str) -> Option<Mesh> {
    let path = geometry_path(map_name)?;
    load_glb(&path)
}

fn geometry_path(map_name: &str) -> Option<PathBuf> {
    if !valid_map_name(map_name) {
        return None;
    }
    let maps = cs2_maps_root()?;
    let binary = vrf_binary();
    if !binary.exists() {
        return None;
    }
    let vpk = maps.join(format!("{map_name}.vpk"));
    if !vpk.exists() {
        return None;
    }
    let root = geometry_root();
    let output = root.join(format!("{map_name}-physics.glb"));
    let metadata = root.join(format!("{map_name}.json"));
    let signature = vpk_signature(&vpk)?;
    if output.exists()
        && fs::read_to_string(&metadata)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            == Some(signature.clone())
    {
        return Some(output);
    }
    extract_physics(&binary, &vpk, map_name, &output, &metadata, signature)
}

fn valid_map_name(map_name: &str) -> bool {
    let Some(suffix) = ["de_", "cs_", "ar_"]
        .into_iter()
        .find_map(|prefix| map_name.strip_prefix(prefix))
    else {
        return false;
    };
    !suffix.is_empty()
        && map_name.len() <= 64
        && suffix
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn extract_physics(
    binary: &Path,
    vpk: &Path,
    map_name: &str,
    output: &Path,
    metadata: &Path,
    signature: serde_json::Value,
) -> Option<PathBuf> {
    let temporary = tempfile_dir(map_name)?;
    let status = Command::new(binary)
        .args([
            "-i",
            &vpk.to_string_lossy(),
            "-o",
            &temporary.to_string_lossy(),
            "-f",
            &format!("maps/{map_name}/world_physics.vmdl_c"),
            "-d",
            "--gltf_export_format",
            "glb",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .ok();
    let generated = find_file(&temporary, "world_physics_physics.glb");
    let result = if status.is_some_and(|code| code.success()) {
        generated
    } else {
        None
    };
    let copied = result.and_then(|generated| {
        fs::create_dir_all(geometry_root()).ok()?;
        fs::copy(&generated, output).ok()?;
        fs::write(metadata, format!("{signature}\n")).ok()?;
        Some(output.to_path_buf())
    });
    let _ = fs::remove_dir_all(&temporary);
    copied
}

fn load_glb(path: &Path) -> Option<Mesh> {
    let bytes = fs::read(path).ok()?;
    let gltf = gltf::Gltf::from_slice(&bytes).ok()?;
    let blob = gltf.blob.as_deref()?;
    let mut triangles = Vec::new();
    for scene in gltf.scenes() {
        for node in scene.nodes() {
            collect_node(node, blob, &mut triangles);
        }
    }
    if triangles.is_empty() {
        return None;
    }
    Some(Mesh::from_triangles(triangles))
}

fn collect_node(node: gltf::Node<'_>, blob: &[u8], triangles: &mut Vec<[[f32; 3]; 3]>) {
    if let Some(mesh) = node.mesh() {
        let mesh_name = mesh.name().unwrap_or_default();
        let node_name = node.name().unwrap_or_default();
        if occludes(mesh_name) || (mesh_name.is_empty() && occludes(node_name)) {
            for primitive in mesh.primitives() {
                collect_primitive(primitive, blob, triangles);
            }
        }
    }
    for child in node.children() {
        collect_node(child, blob, triangles);
    }
}

fn collect_primitive(
    primitive: gltf::Primitive<'_>,
    blob: &[u8],
    triangles: &mut Vec<[[f32; 3]; 3]>,
) {
    if primitive.mode() != gltf::mesh::Mode::Triangles {
        return;
    }
    let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(blob));
    let Some(positions) = reader.read_positions() else {
        return;
    };
    // VRF writes Source-inch vertices and a glTF metres / Y-up node matrix.
    // Demo ticks are Source-space, and Python concatenates local scene geometry,
    // so line-of-sight must use the untransformed positions.
    let positions = positions.collect::<Vec<_>>();
    if let Some(indices) = reader.read_indices() {
        let indices = indices.into_u32().collect::<Vec<_>>();
        for chunk in indices.chunks_exact(3) {
            triangles.push([
                positions[chunk[0] as usize],
                positions[chunk[1] as usize],
                positions[chunk[2] as usize],
            ]);
        }
    } else {
        for chunk in positions.chunks_exact(3) {
            triangles.push([chunk[0], chunk[1], chunk[2]]);
        }
    }
}

fn occludes(name: &str) -> bool {
    let folded = name.to_ascii_lowercase();
    !NON_OCCLUDING.iter().any(|token| folded.contains(token))
}

fn vrf_binary() -> PathBuf {
    data_home().join("omarcs/vrf/Source2Viewer-CLI")
}

fn cs2_maps_root() -> Option<PathBuf> {
    let home = home_dir();
    let candidates = [
        data_home().join("Steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/maps"),
        home.join(".steam/steam/steamapps/common/Counter-Strike Global Offensive/game/csgo/maps"),
    ];
    candidates.into_iter().find(|path| path.exists())
}

fn geometry_root() -> PathBuf {
    data_home().join("omarcs/geometry")
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/share"))
}

fn home_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn vpk_signature(path: &Path) -> Option<serde_json::Value> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let mtime_ns = u64::try_from(modified.duration_since(UNIX_EPOCH).ok()?.as_nanos()).ok()?;
    Some(serde_json::json!({
        "size": meta.len(),
        "mtimeNs": mtime_ns,
    }))
}

fn tempfile_dir(map_name: &str) -> Option<PathBuf> {
    let path = std::env::temp_dir().join(format!("omarcs-{map_name}-{}", std::process::id()));
    fs::create_dir_all(&path).ok()?;
    Some(path)
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, name: &str) -> Option<PathBuf> {
        for entry in fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.is_dir() {
                if let Some(found) = walk(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|file| file == name) {
                return Some(path);
            }
        }
        None
    }
    walk(root, name)
}

fn build_bvh(triangles: &[Triangle]) -> (Vec<BvhNode>, Option<u32>) {
    if triangles.is_empty() {
        return (Vec::new(), None);
    }
    let mut nodes = Vec::with_capacity(triangles.len() * 2);
    let mut indices = (0..triangles.len() as u32).collect::<Vec<_>>();
    let root = build_range(triangles, &mut indices, &mut nodes);
    (nodes, Some(root))
}

fn build_range(triangles: &[Triangle], indices: &mut [u32], nodes: &mut Vec<BvhNode>) -> u32 {
    let bounds = indices
        .iter()
        .map(|index| triangle_bounds(triangles[*index as usize]))
        .reduce(union)
        .expect("range");
    if indices.len() == 1 {
        let id = nodes.len() as u32;
        nodes.push(BvhNode {
            bounds,
            left: indices[0],
            right: u32::MAX,
        });
        return id;
    }
    let axis = longest_axis(bounds);
    indices.sort_by(|left, right| {
        centroid(triangles[*left as usize])[axis]
            .partial_cmp(&centroid(triangles[*right as usize])[axis])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mid = indices.len() / 2;
    let (left_indices, right_indices) = indices.split_at_mut(mid);
    let left = build_range(triangles, left_indices, nodes);
    let right = build_range(triangles, right_indices, nodes);
    let id = nodes.len() as u32;
    nodes.push(BvhNode {
        bounds,
        left,
        right,
    });
    id
}

fn triangle_bounds(triangle: Triangle) -> Aabb {
    Aabb {
        min: [
            triangle.a[0].min(triangle.b[0]).min(triangle.c[0]),
            triangle.a[1].min(triangle.b[1]).min(triangle.c[1]),
            triangle.a[2].min(triangle.b[2]).min(triangle.c[2]),
        ],
        max: [
            triangle.a[0].max(triangle.b[0]).max(triangle.c[0]),
            triangle.a[1].max(triangle.b[1]).max(triangle.c[1]),
            triangle.a[2].max(triangle.b[2]).max(triangle.c[2]),
        ],
    }
}

fn union(left: Aabb, right: Aabb) -> Aabb {
    Aabb {
        min: [
            left.min[0].min(right.min[0]),
            left.min[1].min(right.min[1]),
            left.min[2].min(right.min[2]),
        ],
        max: [
            left.max[0].max(right.max[0]),
            left.max[1].max(right.max[1]),
            left.max[2].max(right.max[2]),
        ],
    }
}

fn longest_axis(bounds: Aabb) -> usize {
    let extents = [
        bounds.max[0] - bounds.min[0],
        bounds.max[1] - bounds.min[1],
        bounds.max[2] - bounds.min[2],
    ];
    if extents[1] > extents[0] && extents[1] >= extents[2] {
        1
    } else if extents[2] > extents[0] && extents[2] >= extents[1] {
        2
    } else {
        0
    }
}

fn centroid(triangle: Triangle) -> [f32; 3] {
    [
        (triangle.a[0] + triangle.b[0] + triangle.c[0]) / 3.0,
        (triangle.a[1] + triangle.b[1] + triangle.c[1]) / 3.0,
        (triangle.a[2] + triangle.b[2] + triangle.c[2]) / 3.0,
    ]
}

fn ray_hits_aabb(origin: [f32; 3], direction: [f32; 3], bounds: Aabb, max_t: f32) -> bool {
    let mut t_min = 0.0_f32;
    let mut t_max = max_t;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-8 {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return false;
            }
            continue;
        }
        let inv = 1.0 / direction[axis];
        let mut t0 = (bounds.min[axis] - origin[axis]) * inv;
        let mut t1 = (bounds.max[axis] - origin[axis]) * inv;
        if t0 > t1 {
            std::mem::swap(&mut t0, &mut t1);
        }
        t_min = t_min.max(t0);
        t_max = t_max.min(t1);
        if t_max < t_min {
            return false;
        }
    }
    true
}

fn triangle_hit(
    origin: [f32; 3],
    direction: [f32; 3],
    triangle: Triangle,
    max_t: f32,
) -> Option<f32> {
    let edge1 = sub(triangle.b, triangle.a);
    let edge2 = sub(triangle.c, triangle.a);
    let pvec = cross(direction, edge2);
    let det = dot(edge1, pvec);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let tvec = sub(origin, triangle.a);
    let u = dot(tvec, pvec) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qvec = cross(tvec, edge1);
    let v = dot(direction, qvec) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = dot(edge2, qvec) * inv;
    (t > 1e-4 && t <= max_t).then_some(t)
}

fn eye(origin: [f32; 3], duck: f32) -> [f32; 3] {
    [origin[0], origin[1], origin[2] + 64.0 - 18.0 * duck]
}

fn view_direction(pitch_degrees: f32, yaw_degrees: f32) -> [f32; 3] {
    let pitch = pitch_degrees.to_radians();
    let yaw = yaw_degrees.to_radians();
    [
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    ]
}

fn sub(first: [f32; 3], second: [f32; 3]) -> [f32; 3] {
    [
        first[0] - second[0],
        first[1] - second[1],
        first[2] - second[2],
    ]
}

fn cross(first: [f32; 3], second: [f32; 3]) -> [f32; 3] {
    [
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ]
}

fn dot(first: [f32; 3], second: [f32; 3]) -> f32 {
    first[0] * second[0] + first[1] * second[1] + first[2] * second[2]
}

fn length(vector: [f32; 3]) -> f32 {
    dot(vector, vector).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(yaw: f32, target: [f32; 3]) -> VisibilityRow {
        VisibilityRow {
            viewer_origin: [0.0, 0.0, 0.0],
            viewer_duck: 0.0,
            viewer_pitch: 0.0,
            viewer_yaw: yaw,
            target_origin: target,
            target_duck: 0.0,
        }
    }

    #[test]
    fn map_geometry_blocks_visibility() {
        let wall = Mesh::axis_aligned_box([1.0, 4.0, 100.0], [5.0, 0.0, 64.0]);
        let rows = [row(0.0, [10.0, 0.0, 0.0]), row(90.0, [0.0, 10.0, 0.0])];
        assert_eq!(visible_rows(&rows, &wall), vec![false, true]);
    }

    #[test]
    fn vrf_export_matrix_keeps_source_space_visibility() {
        let directory = std::env::temp_dir();
        let path = directory.join("omarcs-vrf-source-space.glb");
        std::fs::write(&path, vrf_style_wall_glb()).expect("write glb");
        let mesh = load_glb(&path).expect("load glb");
        std::fs::remove_file(&path).ok();
        let rows = [row(0.0, [10.0, 0.0, 0.0]), row(90.0, [0.0, 10.0, 0.0])];
        assert_eq!(visible_rows(&rows, &mesh), vec![false, true]);
    }

    fn vrf_style_wall_glb() -> Vec<u8> {
        let positions: [f32; 12] = [
            5.0, -2.0, 14.0, 5.0, 2.0, 14.0, 5.0, 2.0, 114.0, 5.0, -2.0, 114.0,
        ];
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let mut bin = Vec::new();
        for value in positions {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        for value in indices {
            bin.extend_from_slice(&value.to_le_bytes());
        }
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0,"matrix":[3.027916e-09,0,0.025399996,0,0.025399996,3.027916e-09,0,0,0,0.025399996,3.027916e-09,0,0,0,0,1]}}],"meshes":[{{"name":"physics_group_concrete","primitives":[{{"attributes":{{"POSITION":0}},"indices":1}}]}}],"accessors":[{{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[5.0,-2.0,14.0],"max":[5.0,2.0,114.0]}},{{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}}],"bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":48}},{{"buffer":0,"byteOffset":48,"byteLength":12}}],"buffers":[{{"byteLength":{}}}]}}"#,
            bin.len()
        );
        glb_bytes(json.into_bytes(), bin)
    }

    fn glb_bytes(mut json: Vec<u8>, mut bin: Vec<u8>) -> Vec<u8> {
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn empty_mesh_leaves_line_of_sight_open() {
        let mesh = Mesh::from_triangles(Vec::<[[f32; 3]; 3]>::new());
        let rows = [row(0.0, [10.0, 0.0, 0.0])];
        assert_eq!(visible_rows(&rows, &mesh), vec![true]);
    }

    #[test]
    fn unknown_map_returns_no_mesh() {
        assert!(load_map_mesh("not-a-map").is_none());
        assert!(load_map_mesh("de_this_map_does_not_exist").is_none());
    }

    #[test]
    fn map_names_are_single_safe_identifiers() {
        for valid in ["de_dust2", "cs_office", "ar_baggage", "de_Map_2"] {
            assert!(valid_map_name(valid), "expected {valid:?} to be valid");
        }
        for invalid in [
            "de_../escape",
            "de_/absolute",
            "de_nested/map",
            "de_nested\\map",
            "de_map.vpk",
            "de_<img src=https://example.test>",
            "de_",
            "de_тест",
        ] {
            assert!(
                !valid_map_name(invalid),
                "expected {invalid:?} to be invalid"
            );
        }
    }

    #[test]
    #[ignore = "loads the local CS2 physics GLB cache"]
    fn loads_cached_nuke_physics_glb() {
        let mesh = load_map_mesh("de_nuke").expect("cached nuke mesh");
        let rows = [row(0.0, [10.0, 0.0, 0.0])];
        assert_eq!(visible_rows(&rows, &mesh).len(), 1);
    }
}
