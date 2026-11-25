//! Process-wide in-memory store for virtual segment files in RAM replay buffer mode.

use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Entries are shared so a save can write them out without holding the lock (and stalling
/// the encoder, which adds the next segment meanwhile).
static STORE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<u8>>>>> = OnceLock::new();

fn store() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<u8>>>> {
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Stores file content under the given path.
pub fn put<P: Into<PathBuf>>(path: P, data: Vec<u8>) {
    let mut guard = store().lock().unwrap();
    guard.insert(path.into(), Arc::new(data));
}

/// Checks whether an entry exists in memory for the given path.
pub fn exists<P: AsRef<Path>>(path: P) -> bool {
    let guard = store().lock().unwrap();
    guard.contains_key(path.as_ref())
}

/// Retrieves a clone of the data for the given path, if present.
#[allow(dead_code)]
pub fn get<P: AsRef<Path>>(path: P) -> Option<Vec<u8>> {
    let guard = store().lock().unwrap();
    guard.get(path.as_ref()).map(|d| d.as_ref().clone())
}

/// Removes an entry from memory. No-op if absent.
pub fn remove<P: AsRef<Path>>(path: P) -> Option<Vec<u8>> {
    let mut guard = store().lock().unwrap();
    guard.remove(path.as_ref()).map(|d| Arc::try_unwrap(d).unwrap_or_else(|d| d.as_ref().clone()))
}

/// Returns the total bytes stored across all virtual files.
#[allow(dead_code)]
pub fn total_bytes() -> usize {
    let guard = store().lock().unwrap();
    guard.values().map(|v| v.len()).sum()
}

/// Writes requested entries that exist in the store to real files at their paths,
/// creating parent directories as needed.
///
/// Returns the list of paths that were written to disk so the caller can delete
/// them after remuxing. The in-memory copies stay in the store.
pub fn materialize<P: AsRef<Path>>(paths: &[P]) -> Result<Vec<PathBuf>> {
    let entries: Vec<(PathBuf, Arc<Vec<u8>>)> = {
        let guard = store().lock().unwrap();
        paths.iter().filter_map(|p| guard.get(p.as_ref()).map(|d| (p.as_ref().to_path_buf(), Arc::clone(d)))).collect()
    };
    let mut materialized = Vec::new();
    for (path, data) in entries {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data.as_slice())?;
        materialized.push(path);
    }
    Ok(materialized)
}

/// Writes every entry currently in the store to its real path on disk and removes
/// it from memory. Returns the paths written.
pub fn spill_all() -> Result<Vec<PathBuf>> {
    let entries: Vec<(PathBuf, Arc<Vec<u8>>)> = {
        let mut guard = store().lock().unwrap();
        guard.drain().collect()
    };
    let mut written = Vec::new();
    for (path, data) in entries {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, data.as_slice())?;
        written.push(path);
    }
    Ok(written)
}

/// Clears all entries from memory, freeing the buffer.
pub fn clear() {
    let mut guard = store().lock().unwrap();
    guard.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn test_put_exists_get_remove() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear();
        let p = PathBuf::from("/tmp/lapse_test_seg_1.mp4");
        put(&p, vec![1, 2, 3, 4]);
        assert!(exists(&p));
        assert_eq!(get(&p), Some(vec![1, 2, 3, 4]));
        assert_eq!(remove(&p), Some(vec![1, 2, 3, 4]));
        assert!(!exists(&p));
        assert_eq!(remove(&p), None);
        clear();
    }

    #[test]
    fn test_total_bytes_and_clear() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear();
        assert_eq!(total_bytes(), 0);
        put(PathBuf::from("/tmp/a.mp4"), vec![0u8; 100]);
        put(PathBuf::from("/tmp/b.mp4"), vec![0u8; 250]);
        assert_eq!(total_bytes(), 350);
        clear();
        assert_eq!(total_bytes(), 0);
    }

    #[test]
    fn test_materialize() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear();
        let dir = std::env::temp_dir().join("lapse_mem_store_test_mat");
        let p1 = dir.join("seg1.mp4");
        let p2 = dir.join("seg1.m4a");
        let p3_absent = dir.join("seg2.mp4");

        put(&p1, vec![10, 20, 30]);
        put(&p2, vec![40, 50]);

        let paths_to_mat = vec![p1.clone(), p2.clone(), p3_absent.clone()];
        let materialized = materialize(&paths_to_mat).expect("materialize failed");

        assert_eq!(materialized.len(), 2);
        assert!(materialized.contains(&p1));
        assert!(materialized.contains(&p2));
        assert!(!materialized.contains(&p3_absent));

        // Disk files exist and match
        assert_eq!(std::fs::read(&p1).unwrap(), vec![10, 20, 30]);
        assert_eq!(std::fs::read(&p2).unwrap(), vec![40, 50]);
        assert!(!p3_absent.exists());

        // Memory store still has copies
        assert!(exists(&p1));
        assert!(exists(&p2));

        // Cleanup
        for p in materialized {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(&dir);
        clear();
    }

    #[test]
    fn test_spill_all() {
        let _lock = TEST_LOCK.lock().unwrap();
        clear();
        let dir = std::env::temp_dir().join("lapse_mem_store_test_spill");
        let p1 = dir.join("segA.mp4");
        let p2 = dir.join("segB.mp4");

        put(&p1, vec![1, 2, 3]);
        put(&p2, vec![4, 5, 6]);

        let spilled = spill_all().expect("spill_all failed");
        assert_eq!(spilled.len(), 2);

        // Files on disk
        assert_eq!(std::fs::read(&p1).unwrap(), vec![1, 2, 3]);
        assert_eq!(std::fs::read(&p2).unwrap(), vec![4, 5, 6]);

        // Memory store should now be empty
        assert_eq!(total_bytes(), 0);
        assert!(!exists(&p1));
        assert!(!exists(&p2));

        // Cleanup
        for p in spilled {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_dir_all(&dir);
        clear();
    }
}
