use std::collections::{HashMap, HashSet};
use serde_json::Value;

const WORKTREE_SEGMENT: &str = "/.claude/worktrees/";
const DEFAULT_PROJECT: &str = "~";

pub fn normalize_project_path(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed == DEFAULT_PROJECT {
        return DEFAULT_PROJECT.to_string();
    }
    if trimmed == "/" {
        return "/".to_string();
    }
    trimmed.trim_end_matches('/').to_string()
}

pub fn project_root_path(raw: &str) -> String {
    let norm = normalize_project_path(raw);
    if let Some(idx) = norm.to_lowercase().find(WORKTREE_SEGMENT) {
        if idx > 0 {
            return norm[..idx].to_string();
        }
    }
    norm
}

pub fn project_name(path: &str) -> String {
    let norm = normalize_project_path(path);
    if norm == DEFAULT_PROJECT {
        return "Workspace".to_string();
    }
    if norm.is_empty() || norm == "/" {
        return norm;
    }
    norm.split('/').last().unwrap_or(&norm).to_string()
}

pub fn path_starts_with(target_path: &str, prefix_path: &str) -> bool {
    let target = normalize_project_path(target_path).to_lowercase();
    let prefix = normalize_project_path(prefix_path).to_lowercase();
    if target.is_empty() || prefix.is_empty() {
        return false;
    }
    if prefix == "/" {
        return target.starts_with('/');
    }
    target == prefix || target.starts_with(&format!("{prefix}/"))
}

pub fn tile_candidate_path(tile: &Value) -> String {
    tile["workspacePath"]
        .as_str()
        .or_else(|| tile["folderPath"].as_str())
        .or_else(|| tile["cwd"].as_str())
        .or_else(|| tile["filePath"].as_str())
        .unwrap_or("")
        .to_string()
}

pub fn tile_project(tile: &Value, projects: &[String]) -> Option<String> {
    if projects.is_empty() {
        return None;
    }
    let normalized: Vec<String> = projects
        .iter()
        .map(|p| normalize_project_path(p))
        .filter(|p| !p.is_empty())
        .collect();

    if let Some(ws) = tile["workspacePath"].as_str() {
        let ws_norm = project_root_path(ws).to_lowercase();
        if let Some(exact) = normalized.iter().find(|p| p.to_lowercase() == ws_norm) {
            return Some(exact.clone());
        }
    }

    let candidate = tile_candidate_path(tile);
    if candidate.is_empty() {
        if tile["type"].as_str() == Some("note") {
            return normalized.into_iter().next();
        }
        return normalized.into_iter().find(|p| p == DEFAULT_PROJECT);
    }

    let mut best: Option<String> = None;
    for proj in &normalized {
        if path_starts_with(&candidate, proj) {
            if best.as_ref().map_or(true, |b: &String| proj.len() > b.len()) {
                best = Some(proj.clone());
            }
        }
    }
    best
}

pub fn normalize_repo(p: &str) -> String {
    p.trim().replace('\\', "/").trim_end_matches('/').to_lowercase()
}

pub fn tile_in_frame(frame: &Value, tile: &Value) -> bool {
    if tile["type"].as_str() == Some("orch") {
        let fp = frame["path"].as_str().unwrap_or("");
        let tc = tile["cwd"].as_str().unwrap_or("");
        if normalize_repo(fp) != normalize_repo(tc) {
            return false;
        }
    }
    let x = tile["x"].as_f64().unwrap_or(0.0);
    let y = tile["y"].as_f64().unwrap_or(0.0);
    let w = tile["width"].as_f64().unwrap_or(0.0);
    let h = tile["height"].as_f64().unwrap_or(0.0);
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    let fx = frame["x"].as_f64().unwrap_or(0.0);
    let fy = frame["y"].as_f64().unwrap_or(0.0);
    let fw = frame["width"].as_f64().unwrap_or(0.0);
    let fh = frame["height"].as_f64().unwrap_or(0.0);
    cx >= fx && cx <= fx + fw && cy >= fy && cy <= fy + fh
}

pub fn frame_of(frames: &[Value], tile: &Value) -> Option<String> {
    let mut best_id: Option<String> = None;
    let mut best_area = f64::INFINITY;
    for frame in frames {
        if !tile_in_frame(frame, tile) {
            continue;
        }
        let fw = frame["width"].as_f64().unwrap_or(0.0);
        let fh = frame["height"].as_f64().unwrap_or(0.0);
        let area = fw * fh;
        if area <= best_area {
            best_id = frame["id"].as_str().map(str::to_string);
            best_area = area;
        }
    }
    best_id
}

fn dominant_project(tiles: &[&Value], projects: &[String]) -> Option<String> {
    let mut order: Vec<String> = Vec::new();
    let mut freq: HashMap<String, usize> = HashMap::new();
    for tile in tiles {
        if let Some(p) = tile_project(tile, projects) {
            if !freq.contains_key(&p) {
                order.push(p.clone());
            }
            *freq.entry(p).or_insert(0) += 1;
        }
    }
    let mut best: Option<String> = None;
    let mut best_count = 0usize;
    for proj in &order {
        let count = freq[proj];
        if count > best_count {
            best = Some(proj.clone());
            best_count = count;
        }
    }
    best
}

#[derive(Debug, Clone, PartialEq)]
pub enum GroupKind {
    Frame,
    Path,
    Loose,
}

impl GroupKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            GroupKind::Frame => "frame",
            GroupKind::Path => "path",
            GroupKind::Loose => "loose",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectGroup {
    pub key: String,
    pub kind: GroupKind,
    pub title: String,
    pub path: Option<String>,
    pub frame_id: Option<String>,
    pub tile_ids: Vec<String>,
    pub outside_ids: Vec<String>,
    pub drifted_ids: Vec<String>,
}

pub struct GroupInputs<'a> {
    pub frames: &'a [Value],
    pub tiles: &'a [Value],
    pub projects: &'a [String],
}

pub fn build_project_groups(input: &GroupInputs) -> Vec<ProjectGroup> {
    let GroupInputs { frames, tiles, projects } = input;

    let mut seen_ids: HashSet<String> = HashSet::new();
    let deduped: Vec<&Value> = tiles
        .iter()
        .filter(|t| seen_ids.insert(t["id"].as_str().unwrap_or("").to_string()))
        .collect();

    let mut frame_groups: Vec<ProjectGroup> = Vec::new();
    let mut fg_path_index: HashMap<String, usize> = HashMap::new();
    let mut framed_projects: HashSet<String> = HashSet::new();

    for frame in frames.iter() {
        let frame_id = frame["id"].as_str().unwrap_or("").to_string();

        let member_tiles: Vec<&Value> = deduped
            .iter()
            .copied()
            .filter(|t| frame_of(frames, t).as_deref() == Some(&frame_id))
            .collect();

        for t in &member_tiles {
            if let Some(p) = tile_project(t, projects) {
                framed_projects.insert(p.to_lowercase());
            }
        }

        let path: Option<String> = frame["path"]
            .as_str()
            .map(normalize_project_path)
            .filter(|s| !s.is_empty())
            .or_else(|| dominant_project(&member_tiles, projects));

        let drifted_ids: Vec<String> = match &path {
            None => Vec::new(),
            Some(p) => {
                let norm_lower = p.to_lowercase();
                member_tiles
                    .iter()
                    .filter_map(|t| {
                        let cand = tile_candidate_path(t);
                        if cand.is_empty() {
                            return None;
                        }
                        let proj = tile_project(t, projects);
                        let is_drifted =
                            proj.as_ref().map_or(true, |q| q.to_lowercase() != norm_lower);
                        if is_drifted {
                            t["id"].as_str().map(str::to_string)
                        } else {
                            None
                        }
                    })
                    .collect()
            }
        };

        let title = {
            let ft = frame["title"].as_str().unwrap_or("");
            if !ft.is_empty() {
                ft.to_string()
            } else {
                match &path {
                    Some(p) => {
                        let n = project_name(p);
                        if n.is_empty() { "Frame".to_string() } else { n }
                    }
                    None => "Frame".to_string(),
                }
            }
        };

        let group_idx = frame_groups.len();
        frame_groups.push(ProjectGroup {
            key: format!("frame:{frame_id}"),
            kind: GroupKind::Frame,
            title,
            path: path.clone(),
            frame_id: Some(frame_id),
            tile_ids: member_tiles
                .iter()
                .filter_map(|t| t["id"].as_str().map(str::to_string))
                .collect(),
            outside_ids: Vec::new(),
            drifted_ids,
        });

        if let Some(p) = path {
            let key = p.to_lowercase();
            fg_path_index.entry(key).or_insert(group_idx);
        }
    }

    let mut path_groups_map: HashMap<String, ProjectGroup> = HashMap::new();
    let mut first_seen_order: Vec<String> = Vec::new();
    let mut loose_group: Option<ProjectGroup> = None;

    for t in &deduped {
        if frame_of(frames, t).is_some() {
            continue;
        }
        match tile_project(t, projects) {
            Some(p) => {
                let proj_lower = p.to_lowercase();
                if let Some(&fg_idx) = fg_path_index.get(&proj_lower) {
                    if let Some(id) = t["id"].as_str() {
                        frame_groups[fg_idx].outside_ids.push(id.to_string());
                    }
                    continue;
                }
                if !path_groups_map.contains_key(&proj_lower) {
                    first_seen_order.push(proj_lower.clone());
                    path_groups_map.insert(
                        proj_lower.clone(),
                        ProjectGroup {
                            key: format!("path:{p}"),
                            kind: GroupKind::Path,
                            title: project_name(&p),
                            path: Some(p.clone()),
                            frame_id: None,
                            tile_ids: Vec::new(),
                            outside_ids: Vec::new(),
                            drifted_ids: Vec::new(),
                        },
                    );
                }
                if let Some(pg) = path_groups_map.get_mut(&proj_lower) {
                    if let Some(id) = t["id"].as_str() {
                        pg.tile_ids.push(id.to_string());
                    }
                }
            }
            None => {
                let lg = loose_group.get_or_insert_with(|| ProjectGroup {
                    key: "loose".to_string(),
                    kind: GroupKind::Loose,
                    title: "Loose".to_string(),
                    path: None,
                    frame_id: None,
                    tile_ids: Vec::new(),
                    outside_ids: Vec::new(),
                    drifted_ids: Vec::new(),
                });
                if let Some(id) = t["id"].as_str() {
                    lg.tile_ids.push(id.to_string());
                }
            }
        }
    }

    for proj in projects.iter() {
        let norm = normalize_project_path(proj);
        if norm.is_empty() {
            continue;
        }
        let norm_lower = norm.to_lowercase();
        if fg_path_index.contains_key(&norm_lower) || framed_projects.contains(&norm_lower) {
            continue;
        }
        if !path_groups_map.contains_key(&norm_lower) {
            path_groups_map.insert(
                norm_lower,
                ProjectGroup {
                    key: format!("path:{norm}"),
                    kind: GroupKind::Path,
                    title: project_name(&norm),
                    path: Some(norm),
                    frame_id: None,
                    tile_ids: Vec::new(),
                    outside_ids: Vec::new(),
                    drifted_ids: Vec::new(),
                },
            );
        }
    }

    let mut result: Vec<ProjectGroup> = frame_groups;
    let mut emitted: HashSet<String> = HashSet::new();

    for proj in projects.iter() {
        let norm_lower = normalize_project_path(proj).to_lowercase();
        if norm_lower.is_empty() || emitted.contains(&norm_lower) {
            continue;
        }
        if let Some(pg) = path_groups_map.remove(&norm_lower) {
            result.push(pg);
            emitted.insert(norm_lower);
        }
    }

    for key in first_seen_order {
        if emitted.contains(&key) {
            continue;
        }
        if let Some(pg) = path_groups_map.remove(&key) {
            result.push(pg);
            emitted.insert(key);
        }
    }

    if let Some(lg) = loose_group {
        result.push(lg);
    }

    result
}

pub fn projects_in_use(frames: &[Value], tiles: &[Value], projects: &[String]) -> Vec<String> {
    projects
        .iter()
        .filter(|project| {
            let key = normalize_project_path(project).to_lowercase();
            if key.is_empty() {
                return false;
            }
            if frames.iter().any(|f| {
                f["path"]
                    .as_str()
                    .map_or(false, |fp| normalize_project_path(fp).to_lowercase() == key)
            }) {
                return true;
            }
            tiles
                .iter()
                .any(|t| tile_project(t, projects).map_or(false, |p| p.to_lowercase() == key))
        })
        .cloned()
        .collect()
}

pub fn sync_project_view_with_tiles(view_projects: &[String], tiles: &[Value]) -> Vec<String> {
    let mut known: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let add = |raw: &str, known: &mut Vec<String>, seen: &mut HashSet<String>| {
        let norm = project_root_path(raw);
        if norm.is_empty() {
            return;
        }
        let key = norm.to_lowercase();
        if seen.insert(key) {
            known.push(norm);
        }
    };

    for p in view_projects {
        add(p, &mut known, &mut seen);
    }
    for t in tiles {
        if let Some(ws) = t["workspacePath"].as_str() {
            add(ws, &mut known, &mut seen);
        }
    }
    if known.is_empty() {
        for t in tiles {
            let raw = t["folderPath"].as_str().or_else(|| t["cwd"].as_str()).unwrap_or("");
            if !raw.is_empty() {
                add(raw, &mut known, &mut seen);
            }
        }
    }
    if known.is_empty() && !tiles.is_empty() {
        add(DEFAULT_PROJECT, &mut known, &mut seen);
    }
    if known.len() > 1 && seen.contains(DEFAULT_PROJECT) {
        let needs_default = tiles.iter().any(|t| {
            tile_project(t, &known).as_deref() == Some(DEFAULT_PROJECT)
                && (t["workspacePath"].as_str() == Some(DEFAULT_PROJECT)
                    || t["cwd"].as_str() == Some(DEFAULT_PROJECT))
        });
        if !needs_default {
            known.retain(|p| p != DEFAULT_PROJECT);
        }
    }
    known
}

#[derive(Debug, Clone)]
pub struct TileGroupInfo {
    pub key: String,
    pub kind: &'static str,
    pub title: String,
    pub path: Option<String>,
    pub index: usize,
    pub order: usize,
    pub outside: bool,
    pub drifted: bool,
}

pub fn assign_group_info(groups: &[ProjectGroup]) -> HashMap<String, TileGroupInfo> {
    let mut map: HashMap<String, TileGroupInfo> = HashMap::new();
    for (group_idx, group) in groups.iter().enumerate() {
        let kind = group.kind.as_str();
        for (order, tile_id) in group.tile_ids.iter().enumerate() {
            let drifted = group.drifted_ids.contains(tile_id);
            map.insert(
                tile_id.clone(),
                TileGroupInfo {
                    key: group.key.clone(),
                    kind,
                    title: group.title.clone(),
                    path: group.path.clone(),
                    index: group_idx,
                    order,
                    outside: false,
                    drifted,
                },
            );
        }
        for (i, tile_id) in group.outside_ids.iter().enumerate() {
            let order = group.tile_ids.len() + i;
            map.insert(
                tile_id.clone(),
                TileGroupInfo {
                    key: group.key.clone(),
                    kind,
                    title: group.title.clone(),
                    path: group.path.clone(),
                    index: group_idx,
                    order,
                    outside: true,
                    drifted: false,
                },
            );
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn term(id: &str, x: f64, y: f64, w: f64, h: f64, cwd: &str) -> Value {
        json!({ "id": id, "type": "term", "x": x, "y": y, "width": w, "height": h, "cwd": cwd })
    }

    fn frame(id: &str, x: f64, y: f64, w: f64, h: f64) -> Value {
        json!({ "id": id, "x": x, "y": y, "width": w, "height": h, "title": "", "color": "#fff" })
    }

    fn frame_path(id: &str, x: f64, y: f64, w: f64, h: f64, path: &str) -> Value {
        json!({ "id": id, "x": x, "y": y, "width": w, "height": h, "title": "", "color": "#fff", "path": path })
    }

    #[test]
    fn smallest_frame_wins() {
        let outer = frame("outer", 0.0, 0.0, 1000.0, 1000.0);
        let inner = frame("inner", 50.0, 50.0, 400.0, 400.0);
        let t = term("t1", 100.0, 100.0, 200.0, 150.0, "/proj");
        let frames = vec![outer, inner];
        assert_eq!(frame_of(&frames, &t).as_deref(), Some("inner"));
    }

    #[test]
    fn path_group_assigned() {
        let t = term("t1", 0.0, 0.0, 10.0, 10.0, "/home/user/myproject/src");
        let projects = vec!["/home/user/myproject".to_string()];
        let groups = build_project_groups(&GroupInputs {
            frames: &[],
            tiles: &[t],
            projects: &projects,
        });
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, GroupKind::Path);
        assert_eq!(groups[0].tile_ids, vec!["t1"]);
    }

    #[test]
    fn loose_group_for_unmatched_tile() {
        let t = term("t1", 0.0, 0.0, 10.0, 10.0, "/other/path");
        let projects = vec!["/proj".to_string()];
        let groups = build_project_groups(&GroupInputs {
            frames: &[],
            tiles: &[t],
            projects: &projects,
        });
        let loose = groups.iter().find(|g| g.kind == GroupKind::Loose).unwrap();
        assert!(loose.tile_ids.contains(&"t1".to_string()));
    }

    #[test]
    fn outside_frame_tile_in_frame_group() {
        let f = frame_path("f1", 0.0, 0.0, 100.0, 100.0, "/proj");
        let inside = term("t-in", 10.0, 10.0, 20.0, 20.0, "/proj/a");
        let outside = term("t-out", 500.0, 500.0, 20.0, 20.0, "/proj/b");
        let projects = vec!["/proj".to_string()];
        let groups = build_project_groups(&GroupInputs {
            frames: &[f],
            tiles: &[inside, outside],
            projects: &projects,
        });
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].kind, GroupKind::Frame);
        assert!(groups[0].tile_ids.contains(&"t-in".to_string()));
        assert!(groups[0].outside_ids.contains(&"t-out".to_string()));
    }

    #[test]
    fn group_ordering_frames_then_projects_then_loose() {
        let f = frame_path("f1", 0.0, 0.0, 200.0, 200.0, "/proj-a");
        let t_frame = term("t-frame", 10.0, 10.0, 20.0, 20.0, "/proj-a/x");
        let t_proj_c = term("t-c", 600.0, 0.0, 10.0, 10.0, "/proj-c/x");
        let t_proj_b = term("t-b", 500.0, 0.0, 10.0, 10.0, "/proj-b/x");
        let t_loose = term("t-loose", 700.0, 0.0, 10.0, 10.0, "/unknown");
        let projects = vec!["/proj-a".to_string(), "/proj-b".to_string(), "/proj-c".to_string()];
        let tiles = vec![t_frame, t_proj_c, t_proj_b, t_loose];
        let groups = build_project_groups(&GroupInputs {
            frames: &[f],
            tiles: &tiles,
            projects: &projects,
        });
        assert_eq!(groups[0].kind, GroupKind::Frame);
        assert_eq!(groups[1].key, "path:/proj-b");
        assert_eq!(groups[2].key, "path:/proj-c");
        assert_eq!(groups[3].kind, GroupKind::Loose);
    }

    #[test]
    fn note_no_path_goes_to_first_project() {
        let note = json!({ "id": "n1", "type": "note", "x": 0.0, "y": 0.0, "width": 10.0, "height": 10.0 });
        let projects = vec!["/first-proj".to_string(), "/second-proj".to_string()];
        let proj = tile_project(&note, &projects);
        assert_eq!(proj.as_deref(), Some("/first-proj"));
    }
}
