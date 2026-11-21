//! Visually-similar image detection (V2): perceptual hashes (pHash), grouped by Hamming distance.
//!
//! This is deliberately separate from the exact (SHA-256) pipeline: it never decides anything
//! about byte-identity, it only reports images that *look* alike. Fingerprints are cached in
//! SQLite and invalidated when a file's size or mtime changes, like the exact hashes.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use image::imageops::FilterType;
use rusqlite::{params, Connection};

use crate::scan::{mtime_ns, DupFile, DupGroup};

const SETTING_ENABLED: &str = "scan.similar";
const SETTING_SENSITIVITY: &str = "similar.sensitivity";
const FP_BATCH: usize = 100;
const MAX_WORKERS: usize = 8;

// ---------- Settings ----------

/// How close two fingerprints must be (maximum differing bits out of 64) to count as similar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensitivity {
    Strict,
    Normal,
    Loose,
}

impl Sensitivity {
    pub fn threshold(self) -> u32 {
        match self {
            Sensitivity::Strict => 3,
            Sensitivity::Normal => 6,
            Sensitivity::Loose => 10,
        }
    }

    fn parse(s: &str) -> Option<Sensitivity> {
        match s {
            "strict" => Some(Sensitivity::Strict),
            "normal" => Some(Sensitivity::Normal),
            "loose" => Some(Sensitivity::Loose),
            _ => None,
        }
    }
}

/// Whether scans compute fingerprints. On unless the user turned it off.
pub fn enabled(conn: &Connection) -> Result<bool, String> {
    Ok(crate::db::get_setting(conn, SETTING_ENABLED)?.as_deref() != Some("0"))
}

pub fn sensitivity(conn: &Connection) -> Result<Sensitivity, String> {
    Ok(crate::db::get_setting(conn, SETTING_SENSITIVITY)?
        .as_deref()
        .and_then(Sensitivity::parse)
        .unwrap_or(Sensitivity::Normal))
}

/// Maximum Hamming distance currently configured.
pub fn threshold(conn: &Connection) -> Result<u32, String> {
    Ok(sensitivity(conn)?.threshold())
}

// ---------- Perceptual hash ----------

const N: usize = 32; // the image is reduced to N x N grayscale
const LOW: usize = 8; // the LOW x LOW lowest DCT frequencies make up the 64-bit hash

/// cos(pi * (2x + 1) * k / 2N) for the first LOW frequencies.
fn cos_table() -> [[f32; N]; LOW] {
    let mut t = [[0f32; N]; LOW];
    for (k, row) in t.iter_mut().enumerate() {
        for (x, v) in row.iter_mut().enumerate() {
            *v = (std::f32::consts::PI * (2 * x + 1) as f32 * k as f32 / (2 * N) as f32).cos();
        }
    }
    t
}

/// pHash of a decoded image: 32x32 grayscale → 2D DCT → the 8x8 low frequencies, thresholded
/// at their median. Robust to resizing, recompression and small colour or brightness changes.
pub fn phash(img: &image::DynamicImage) -> u64 {
    let gray = img.to_luma8();
    let small = image::imageops::resize(&gray, N as u32, N as u32, FilterType::Triangle);
    let px = |x: usize, y: usize| small.get_pixel(x as u32, y as u32).0[0] as f32;

    let c = cos_table();
    // Rows first (only the LOW frequencies we keep), then columns.
    let mut tmp = [[0f32; LOW]; N];
    for (y, row) in tmp.iter_mut().enumerate() {
        for (k, v) in row.iter_mut().enumerate() {
            *v = (0..N).map(|x| px(x, y) * c[k][x]).sum();
        }
    }
    let mut coef = [0f32; LOW * LOW];
    for l in 0..LOW {
        for k in 0..LOW {
            coef[l * LOW + k] = (0..N).map(|y| tmp[y][k] * c[l][y]).sum();
        }
    }

    // The DC term (overall brightness) would skew the median, so leave it out of it.
    let mut sorted: Vec<f32> = coef[1..].to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[sorted.len() / 2];

    coef.iter()
        .enumerate()
        .fold(0u64, |h, (i, &v)| if v > median { h | (1 << i) } else { h })
}

/// Largest side the sharpness measure looks at, so scores compare across resolutions.
const SHARPNESS_SIDE: u32 = 512;

/// Sharpness score: variance of the Laplacian of the grayscale picture, reduced to at most
/// `SHARPNESS_SIDE` pixels on its long side. Blurry photos score low, detailed ones high.
pub fn sharpness(img: &image::DynamicImage) -> f64 {
    let gray = img.to_luma8();
    let (w, h) = gray.dimensions();
    let gray = if w.max(h) > SHARPNESS_SIDE {
        let scale = SHARPNESS_SIDE as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
        image::imageops::resize(&gray, nw, nh, FilterType::Triangle)
    } else {
        gray
    };
    let (w, h) = gray.dimensions();
    if w < 3 || h < 3 {
        return 0.0;
    }
    let px = |x: u32, y: u32| gray.get_pixel(x, y).0[0] as f64;
    let (mut sum, mut sum_sq, mut n) = (0.0, 0.0, 0.0);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let lap = px(x - 1, y) + px(x + 1, y) + px(x, y - 1) + px(x, y + 1) - 4.0 * px(x, y);
            sum += lap;
            sum_sq += lap * lap;
            n += 1.0;
        }
    }
    let mean = sum / n;
    sum_sq / n - mean * mean
}

/// Decodes once and returns both the perceptual hash and the sharpness score.
pub fn analyse_file(path: &str) -> Result<(u64, f64), image::ImageError> {
    let img = image::open(path)?;
    Ok((phash(&img), sharpness(&img)))
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

// ---------- Fingerprinting (scan phase) ----------

pub struct FpCandidate {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
}

/// Files seen in `scan_id` that have no fingerprint yet (and haven't failed to decode), plus
/// fingerprinted ones from before sharpness was recorded.
pub fn candidates(conn: &Connection, scan_id: i64) -> Result<Vec<FpCandidate>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, path, size, mtime_ns FROM files
             WHERE last_seen_scan = ?1
               AND ((phash IS NULL AND phash_failed = 0) OR (phash IS NOT NULL AND sharpness IS NULL))",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![scan_id], |r| {
            Ok(FpCandidate { id: r.get(0)?, path: r.get(1)?, size: r.get(2)?, mtime: r.get(3)? })
        })
        .map_err(|e| e.to_string())?;
    // Only images can be fingerprinted; other file types never get a hash.
    let all: Vec<FpCandidate> = rows.collect::<Result<_, _>>().map_err(|e| e.to_string())?;
    Ok(all.into_iter().filter(|c| crate::scan::is_image_path(Path::new(&c.path))).collect())
}

enum Outcome {
    Hash(u64, f64),
    Undecodable,
    /// Changed while we were reading it: try again next scan.
    Changed,
}

fn fingerprint_one(c: &FpCandidate) -> Outcome {
    let result = analyse_file(&c.path);
    let unchanged = fs::metadata(&c.path)
        .map(|m| m.len() as i64 == c.size && mtime_ns(&m) == c.mtime)
        .unwrap_or(false);
    match (result, unchanged) {
        (_, false) => Outcome::Changed,
        (Ok((h, s)), true) => Outcome::Hash(h, s),
        (Err(_), true) => Outcome::Undecodable,
    }
}

type Analysed = Option<(u64, f64)>;

fn flush(conn: &mut Connection, pending: &mut Vec<(i64, Analysed)>) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    {
        let mut stmt = tx
            .prepare_cached("UPDATE files SET phash = ?2, phash_failed = ?3, sharpness = ?4 WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        for (id, h) in pending.iter() {
            // SQLite integers are signed: store the bit pattern.
            stmt.execute(params![id, h.map(|(h, _)| h as i64), h.is_none() as i64, h.map(|(_, s)| s)])
                .map_err(|e| e.to_string())?;
        }
    }
    tx.commit().map_err(|e| e.to_string())?;
    pending.clear();
    Ok(())
}

/// Computes fingerprints on a few worker threads and stores them. Returns how many were
/// stored and whether the run was cancelled. Finished work is kept even when cancelled.
pub fn fingerprint(
    conn: &mut Connection,
    todo: Vec<FpCandidate>,
    cancel: &AtomicBool,
    on_done: &mut dyn FnMut(u64),
) -> Result<(u64, bool), String> {
    let workers = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(1))
        .unwrap_or(1)
        .clamp(1, MAX_WORKERS)
        .min(todo.len().max(1));
    let next = AtomicUsize::new(0);
    let (tx, rx) = mpsc::channel::<(i64, Outcome)>();

    let mut stored = 0u64;
    let mut done = 0u64;
    let mut pending: Vec<(i64, Analysed)> = Vec::with_capacity(FP_BATCH);
    let mut failure: Option<String> = None;

    std::thread::scope(|s| {
        for _ in 0..workers {
            let tx = tx.clone();
            let (todo, next) = (&todo, &next);
            s.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(c) = todo.get(i) else { return };
                    if tx.send((c.id, fingerprint_one(c))).is_err() {
                        return;
                    }
                }
            });
        }
        drop(tx); // the loop below ends once every worker has dropped its sender

        for (id, outcome) in rx {
            done += 1;
            match outcome {
                Outcome::Hash(h, s) => pending.push((id, Some((h, s)))),
                Outcome::Undecodable => pending.push((id, None)),
                Outcome::Changed => {}
            }
            if pending.len() >= FP_BATCH && failure.is_none() {
                stored += pending.len() as u64;
                if let Err(e) = flush(conn, &mut pending) {
                    failure = Some(e);
                    cancel_workers(cancel);
                }
            }
            on_done(done);
        }
    });

    if let Some(e) = failure {
        return Err(e);
    }
    stored += pending.len() as u64;
    flush(conn, &mut pending)?;
    Ok((stored, cancel.load(Ordering::Relaxed)))
}

/// Stops the workers after a database error. (The caller's flag is also what the user's
/// cancel button sets, which is fine: a failed scan is over either way.)
fn cancel_workers(cancel: &AtomicBool) {
    cancel.store(true, Ordering::Relaxed);
}

// ---------- Clustering ----------

/// BK-tree over 64-bit hashes under Hamming distance, for radius queries without
/// comparing every pair.
struct BkTree {
    /// (hash, children as (distance to this node, node index))
    nodes: Vec<(u64, Vec<(u32, usize)>)>,
}

impl BkTree {
    fn new() -> Self {
        BkTree { nodes: Vec::new() }
    }

    fn insert(&mut self, h: u64) -> usize {
        let new = self.nodes.len();
        if new == 0 {
            self.nodes.push((h, Vec::new()));
            return 0;
        }
        let mut cur = 0;
        loop {
            let d = hamming(self.nodes[cur].0, h);
            match self.nodes[cur].1.iter().find(|(cd, _)| *cd == d) {
                Some(&(_, child)) => cur = child,
                None => {
                    self.nodes[cur].1.push((d, new));
                    self.nodes.push((h, Vec::new()));
                    return new;
                }
            }
        }
    }

    fn within(&self, h: u64, radius: u32, out: &mut Vec<usize>) {
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let d = hamming(self.nodes[n].0, h);
            if d <= radius {
                out.push(n);
            }
            for &(cd, child) in &self.nodes[n].1 {
                if cd + radius >= d && cd <= d + radius {
                    stack.push(child);
                }
            }
        }
    }
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Groups item indexes whose hashes are within `max_dist` of each other (directly or through
/// a chain of such neighbours). Only groups of two or more items are returned.
pub fn cluster(hashes: &[u64], max_dist: u32) -> Vec<Vec<usize>> {
    // Identical hashes are the same node; most of a library's duplicates collapse here.
    let mut unique: Vec<u64> = Vec::new();
    let mut slot: HashMap<u64, usize> = HashMap::new();
    let mut members: Vec<Vec<usize>> = Vec::new();
    for (i, &h) in hashes.iter().enumerate() {
        let s = *slot.entry(h).or_insert_with(|| {
            unique.push(h);
            members.push(Vec::new());
            unique.len() - 1
        });
        members[s].push(i);
    }

    let mut parent: Vec<usize> = (0..unique.len()).collect();
    let mut tree = BkTree::new();
    let mut hits = Vec::new();
    for (u, &h) in unique.iter().enumerate() {
        hits.clear();
        tree.within(h, max_dist, &mut hits);
        for &other in &hits {
            // The tree's node index equals the unique index because we insert in order.
            let (a, b) = (find(&mut parent, u), find(&mut parent, other));
            if a != b {
                parent[a] = b;
            }
        }
        tree.insert(h);
    }

    let mut by_root: HashMap<usize, Vec<usize>> = HashMap::new();
    for u in 0..unique.len() {
        let r = find(&mut parent, u);
        by_root.entry(r).or_default().extend(&members[u]);
    }
    let mut groups: Vec<Vec<usize>> = by_root.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups {
        g.sort_unstable();
    }
    groups.sort_unstable();
    groups
}

// ---------- Listing ----------

struct Row {
    file: DupFile,
    sha: Option<String>,
    phash: u64,
}

/// Groups of visually similar images, excluding groups whose files are all byte-identical
/// (those already appear as exact duplicates). Hard links to one physical file count once.
pub fn similar_groups(conn: &Connection) -> Result<Vec<DupGroup>, String> {
    let max_dist = threshold(conn)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, path, size, sha256, file_id, mtime_ns, width, height, phash, sharpness
             FROM files WHERE phash IS NOT NULL ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<u32>>(6)?,
                r.get::<_, Option<u32>>(7)?,
                r.get::<_, i64>(8)? as u64,
                r.get::<_, Option<f64>>(9)?,
            ))
        })
        .map_err(|e| e.to_string())?;

    let mut all: Vec<(Row, Option<String>)> = Vec::new();
    for row in rows {
        let (id, path, size, sha, fid, mtime, width, height, ph, sharp) = row.map_err(|e| e.to_string())?;
        all.push((
            Row {
                file: DupFile {
                    id,
                    path,
                    size,
                    modified: mtime / 1_000_000_000,
                    created: None,
                    width,
                    height,
                    sharpness: sharp,
                    file_id: fid.clone(),
                },
                sha,
                phash: ph,
            },
            fid,
        ));
    }

    let hashes: Vec<u64> = all.iter().map(|(r, _)| r.phash).collect();
    let mut out = Vec::new();
    for idxs in cluster(&hashes, max_dist) {
        // Only now touch the disk, and only for files that ended up in a candidate group.
        let members: Vec<usize> = idxs
            .into_iter()
            .filter(|&i| Path::new(&all[i].0.file.path).is_file())
            .collect();
        if members.len() < 2 {
            continue;
        }
        let mut physical: Vec<&str> = Vec::new();
        let mut unknown = 0usize;
        for &i in &members {
            match &all[i].1 {
                Some(f) if !physical.contains(&f.as_str()) => physical.push(f),
                Some(_) => {}
                None => unknown += 1,
            }
        }
        if physical.len() + unknown < 2 {
            continue;
        }
        let first_sha = all[members[0]].0.sha.as_deref();
        if first_sha.is_some() && members.iter().all(|&i| all[i].0.sha.as_deref() == first_sha) {
            continue; // every file is byte-identical: the exact view covers this
        }

        let id = members.iter().map(|&i| all[i].0.file.id).min().unwrap_or(0);
        let files: Vec<DupFile> = members
            .iter()
            .map(|&i| {
                let f = &all[i].0.file;
                DupFile {
                    id: f.id,
                    path: f.path.clone(),
                    size: f.size,
                    modified: f.modified,
                    created: None,
                    width: f.width,
                    height: f.height,
                    sharpness: f.sharpness,
                    file_id: f.file_id.clone(),
                }
            })
            .collect();
        out.push(DupGroup {
            kind: "similar",
            sha256: format!("similar-{id}"),
            size: files.iter().map(|f| f.size).max().unwrap_or(0),
            wasted: 0, // needs the recommended copy: filled by `review::enrich_similar`
            keep_id: 0,
            files,
        });
    }
    Ok(out)
}

/// Bytes freed by keeping only `keep_id`: every other *physical* file (hard links count once).
pub fn wasted_bytes(files: &[DupFile], keep_id: i64) -> i64 {
    let keep_fid = files.iter().find(|f| f.id == keep_id).and_then(|f| f.file_id.as_deref());
    let mut seen: Vec<&str> = Vec::new();
    let mut total = 0;
    for f in files.iter().filter(|f| f.id != keep_id) {
        match f.file_id.as_deref() {
            Some(fid) if Some(fid) == keep_fid || seen.contains(&fid) => {}
            Some(fid) => {
                seen.push(fid);
                total += f.size;
            }
            None => total += f.size,
        }
    }
    total
}

/// Number of similar groups, for the dashboard.
pub fn group_count(conn: &Connection) -> Result<i64, String> {
    Ok(similar_groups(conn)?.len() as i64)
}

#[cfg(test)]
/// A photo-like picture: many spatial frequencies whose strength falls off as 1/f
/// (as in natural images), with phases that differ for each `seed`.
pub(crate) fn test_picture(seed: u32, size: u32) -> image::DynamicImage {
    let mut state = seed.wrapping_mul(2654435761).wrapping_add(12345);
    let mut rnd = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        (state % 10_000) as f32 / 10_000.0
    };
    let mut waves = Vec::new();
    for kx in 0..7 {
        for ky in 0..7 {
            if kx + ky == 0 {
                continue;
            }
            let amp = 1.0 / ((kx * kx + ky * ky) as f32).sqrt();
            waves.push((kx as f32, ky as f32, amp, rnd() * 6.28, rnd() * 6.28, rnd() * 6.28));
        }
    }
    let mut img = image::RgbImage::new(size, size);
    let s = size as f32;
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32 / s, y as f32 / s);
        let mut c = [0f32; 3];
        for &(kx, ky, amp, p0, p1, p2) in &waves {
            let arg = 6.28 * (kx * fx + ky * fy);
            c[0] += amp * (arg + p0).sin();
            c[1] += amp * (arg + p1).sin();
            c[2] += amp * (arg + p2).sin();
        }
        *p = image::Rgb(c.map(|v| (128.0 + 40.0 * v).clamp(0.0, 255.0) as u8));
    }
    image::DynamicImage::ImageRgb8(img)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::test_picture as picture;

    #[test]
    fn same_picture_at_other_sizes_is_close() {
        let big = picture(1, 256);
        let small = big.resize_exact(80, 80, FilterType::Lanczos3);
        assert!(hamming(phash(&big), phash(&small)) <= 3);
    }

    #[test]
    fn brightness_shift_is_close() {
        let a = picture(2, 128);
        let b = a.brighten(20);
        assert!(hamming(phash(&a), phash(&b)) <= 4);
    }

    #[test]
    fn different_pictures_are_far() {
        let h: Vec<u64> = (1..=5).map(|s| phash(&picture(s, 128))).collect();
        for i in 0..h.len() {
            for j in i + 1..h.len() {
                assert!(hamming(h[i], h[j]) > 10, "{i} vs {j}: {}", hamming(h[i], h[j]));
            }
        }
    }

    #[test]
    fn blur_lowers_sharpness() {
        // Photo-like structure plus fine, deterministic texture (so blur has detail to remove).
        let mut rgb = picture(1, 256).to_rgb8();
        for (x, y, p) in rgb.enumerate_pixels_mut() {
            let n = ((x * 7919 + y * 104729) % 41) as i16 - 20;
            for c in p.0.iter_mut() {
                *c = (*c as i16 + n).clamp(0, 255) as u8;
            }
        }
        let sharp = image::DynamicImage::ImageRgb8(rgb);
        let blurred = sharp.blur(3.0);
        assert!(sharpness(&sharp) > sharpness(&blurred) * 2.0);
        assert_eq!(sharpness(&image::DynamicImage::new_rgb8(2, 2)), 0.0);
    }

    #[test]
    fn scan_records_sharpness_and_backfills_old_fingerprints() {
        let (mut conn, dir) = env();
        picture(1, 64).save(dir.path().join("a.png")).unwrap();
        scan(&mut conn);
        let sharp: Option<f64> =
            conn.query_row("SELECT sharpness FROM files", [], |r| r.get(0)).unwrap();
        assert!(sharp.is_some());
        // A fingerprint from before sharpness existed gets filled in by the next scan.
        conn.execute("UPDATE files SET sharpness = NULL", []).unwrap();
        assert_eq!(scan(&mut conn).files_fingerprinted, 1);
        let sharp: Option<f64> =
            conn.query_row("SELECT sharpness FROM files", [], |r| r.get(0)).unwrap();
        assert!(sharp.is_some());
    }

    #[test]
    fn hamming_counts_bits() {
        assert_eq!(hamming(0, 0), 0);
        assert_eq!(hamming(0b1011, 0b0001), 2);
        assert_eq!(hamming(u64::MAX, 0), 64);
    }

    #[test]
    fn cluster_groups_within_threshold_and_chains() {
        // 0 and 1 differ by 2 bits, 1 and 2 by 2 bits (0 and 2 by 4): one chain at radius 2.
        let hashes = [0b0000u64, 0b0011, 0b1111, u64::MAX, 0b0000];
        let mut groups = cluster(&hashes, 2);
        groups.sort();
        assert_eq!(groups, vec![vec![0, 1, 2, 4]]);
        // A tighter radius splits the chain; the identical pair (0, 4) remains.
        assert_eq!(cluster(&hashes, 1), vec![vec![0, 4]]);
    }

    #[test]
    fn cluster_matches_brute_force() {
        // Deterministic pseudo-random hashes, some deliberately near each other.
        let mut x = 0x9E3779B97F4A7C15u64;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut hashes = Vec::new();
        for _ in 0..150 {
            let base = next();
            hashes.push(base);
            hashes.push(base ^ (1 << (next() % 64)));
        }
        let fast = cluster(&hashes, 3);

        // Brute-force union-find as the reference.
        let mut parent: Vec<usize> = (0..hashes.len()).collect();
        for i in 0..hashes.len() {
            for j in i + 1..hashes.len() {
                if hamming(hashes[i], hashes[j]) <= 3 {
                    let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                    parent[a] = b;
                }
            }
        }
        let mut by: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..hashes.len() {
            let r = find(&mut parent, i);
            by.entry(r).or_default().push(i);
        }
        let mut slow: Vec<Vec<usize>> = by.into_values().filter(|g| g.len() > 1).collect();
        slow.sort();
        assert_eq!(fast, slow);
    }

    #[test]
    fn wasted_counts_hard_links_once() {
        let f = |id, size, fid: Option<&str>| DupFile {
            id,
            path: String::new(),
            size,
            modified: 0,
            created: None,
            width: None,
            height: None,
            sharpness: None,
            file_id: fid.map(String::from),
        };
        let files = [f(1, 100, Some("a")), f(2, 100, Some("a")), f(3, 40, Some("b")), f(4, 30, None)];
        // Keeping 1: its hard link (2) frees nothing; 3 and 4 free 70.
        assert_eq!(wasted_bytes(&files, 1), 70);
        // Keeping 3: one physical copy of "a" (100) plus the unlinked 30.
        assert_eq!(wasted_bytes(&files, 3), 130);
    }

    fn env() -> (Connection, tempfile::TempDir) {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::db::add_folder(&conn, dir.path().to_str().unwrap()).unwrap();
        (conn, dir)
    }

    fn scan(conn: &mut Connection) -> crate::scan::Summary {
        let id = crate::scan::begin_scan(conn).unwrap();
        crate::scan::run(conn, id, &AtomicBool::new(false), &crate::scan::Config::load(conn).unwrap(), &mut |_| {})
            .unwrap()
    }

    #[test]
    fn scan_finds_resized_and_recompressed_copies_but_not_other_pictures() {
        let (mut conn, dir) = env();
        let base = picture(1, 256);
        base.save(dir.path().join("original.png")).unwrap();
        base.resize_exact(96, 96, FilterType::Lanczos3).save(dir.path().join("small.png")).unwrap();
        base.save(dir.path().join("recompressed.jpg")).unwrap();
        picture(4, 256).save(dir.path().join("unrelated.png")).unwrap();

        let s = scan(&mut conn);
        assert_eq!(s.files_fingerprinted, 4);

        let groups = similar_groups(&conn).unwrap();
        assert_eq!(groups.len(), 1);
        let names: Vec<String> = groups[0]
            .files
            .iter()
            .map(|f| Path::new(&f.path).file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 3, "{names:?}");
        assert!(!names.contains(&"unrelated.png".to_string()));
    }

    #[test]
    fn byte_identical_groups_are_left_to_the_exact_view() {
        let (mut conn, dir) = env();
        let img = picture(2, 64);
        img.save(dir.path().join("a.png")).unwrap();
        fs::copy(dir.path().join("a.png"), dir.path().join("b.png")).unwrap();
        scan(&mut conn);
        assert!(similar_groups(&conn).unwrap().is_empty());
        assert_eq!(crate::scan::duplicate_groups(&conn).unwrap().len(), 1);
    }

    #[test]
    fn fingerprints_are_cached_and_invalidated_by_changes() {
        let (mut conn, dir) = env();
        let p = dir.path().join("a.png");
        picture(1, 64).save(&p).unwrap();
        assert_eq!(scan(&mut conn).files_fingerprinted, 1);
        assert_eq!(scan(&mut conn).files_fingerprinted, 0, "cached on the second scan");

        picture(3, 96).save(&p).unwrap(); // different size and content
        assert_eq!(scan(&mut conn).files_fingerprinted, 1, "recomputed after a change");
    }

    #[test]
    fn undecodable_images_are_not_retried() {
        let (mut conn, dir) = env();
        fs::write(dir.path().join("broken.png"), b"not really a png").unwrap();
        assert_eq!(scan(&mut conn).files_fingerprinted, 1);
        assert_eq!(scan(&mut conn).files_fingerprinted, 0);
        assert!(similar_groups(&conn).unwrap().is_empty());
    }

    #[test]
    fn disabling_the_setting_skips_fingerprinting() {
        let (mut conn, dir) = env();
        crate::db::set_setting(&conn, SETTING_ENABLED, "0").unwrap();
        picture(1, 64).save(dir.path().join("a.png")).unwrap();
        assert_eq!(scan(&mut conn).files_fingerprinted, 0);
    }

    #[test]
    fn sensitivity_defaults_to_normal() {
        let conn = crate::db::init(Connection::open_in_memory().unwrap()).unwrap();
        assert_eq!(sensitivity(&conn).unwrap(), Sensitivity::Normal);
        crate::db::set_setting(&conn, SETTING_SENSITIVITY, "loose").unwrap();
        assert_eq!(threshold(&conn).unwrap(), 10);
        crate::db::set_setting(&conn, SETTING_SENSITIVITY, "bogus").unwrap();
        assert_eq!(sensitivity(&conn).unwrap(), Sensitivity::Normal);
    }
}
