use crate::config::Limits;
use crate::db::Db;
use citrine_gb_net::ErrorCode;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const WARN_EVERY: Duration = Duration::from_secs(60);

pub fn charge(size: u64) -> u64 {
    citrine_gb_net::storage_cost(size)
}

pub struct Storage {
    data_dir: PathBuf,
    floor_bytes: u64,
    floor_percent: u64,
    reserved: AtomicU64,
    warned: Mutex<Option<Instant>>,
}

pub struct Reservation {
    storage: Arc<Storage>,
    bytes: u64,
}

impl Reservation {
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.storage
            .reserved
            .fetch_sub(self.bytes, Ordering::Relaxed);
    }
}

impl Storage {
    pub fn new(data_dir: PathBuf, limits: &Limits) -> Self {
        Self {
            data_dir,
            floor_bytes: limits.disk_floor_bytes,
            floor_percent: limits.disk_floor_percent,
            reserved: AtomicU64::new(0),
            warned: Mutex::new(None),
        }
    }

    pub fn reserve(self: &Arc<Self>, bytes: u64) -> Reservation {
        self.reserved.fetch_add(bytes, Ordering::Relaxed);
        Reservation {
            storage: self.clone(),
            bytes,
        }
    }

    fn warn(&self, why: &str) {
        let mut warned = self.warned.lock().unwrap_or_else(|e| e.into_inner());
        if warned.is_none_or(|at| at.elapsed() > WARN_EVERY) {
            *warned = Some(Instant::now());
            log::warn!("refused a write: {why}");
        }
    }

    pub fn floor(&self, disk_total: u64) -> u64 {
        self.floor_bytes.max(disk_total / 100 * self.floor_percent)
    }

    pub fn disk_room(&self) -> Option<u64> {
        let stats = fs4::statvfs(&self.data_dir).ok()?;
        Some(
            stats
                .available_space()
                .saturating_sub(self.floor(stats.total_space())),
        )
    }

    pub async fn used(db: &Db) -> Result<u64, sqlx::Error> {
        Ok(db.total_storage().await? + db.size().await?)
    }

    pub async fn room(&self, db: &Db, adding: u64, total: u64) -> Result<(), ErrorCode> {
        let server = |e: sqlx::Error| {
            log::error!("database: {e}");
            ErrorCode::Server
        };
        let used = Self::used(db).await.map_err(server)? + self.reserved.load(Ordering::Relaxed);
        if used.saturating_add(adding) > total {
            self.warn(&format!(
                "the relay's storage is at its cap of {} MB (Total storage in the admin panel, or CITRINE_STORAGE_TOTAL_MB)",
                total >> 20
            ));
            return Err(ErrorCode::QuotaFull);
        }
        let stats = fs4::statvfs(&self.data_dir).map_err(|e| {
            log::error!("free disk space: {e}");
            ErrorCode::Server
        })?;
        let floor = self.floor(stats.total_space());
        if stats.available_space() < floor.saturating_add(adding) {
            self.warn(&format!(
                "the disk has less than {} MB free (CITRINE_DISK_FLOOR_MB, CITRINE_DISK_FLOOR_PERCENT)",
                floor >> 20
            ));
            return Err(ErrorCode::QuotaFull);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_files_cost_a_block() {
        assert_eq!(charge(1), 4096);
        assert_eq!(charge(4096), 4096);
        assert_eq!(charge(5000), 5000);
    }

    #[test]
    fn the_floor_is_the_larger_of_size_and_share() {
        let limits = Limits::from_vars(|_| None).unwrap();
        let storage = Storage::new(PathBuf::from("."), &limits);
        assert_eq!(storage.floor(10 << 30), 2 << 30);
        assert_eq!(storage.floor(100 << 30), 5 << 30);
    }

    #[test]
    fn reservations_count_until_dropped() {
        let limits = Limits::from_vars(|_| None).unwrap();
        let storage = Arc::new(Storage::new(PathBuf::from("."), &limits));
        let a = storage.reserve(100);
        let b = storage.reserve(50);
        assert_eq!(storage.reserved.load(Ordering::Relaxed), 150);
        drop(a);
        assert_eq!(storage.reserved.load(Ordering::Relaxed), 50);
        assert_eq!(b.bytes(), 50);
    }
}
