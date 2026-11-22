//! 任务的改动统计。在大仓库里要算好几秒，所以在后台算，调用方（拿着引擎的锁）只取最近一次的结果。

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use duolo_core::TaskId;
use duolo_git::DiffStat;

/// 结果多久算一次。
const REFRESH: Duration = Duration::from_secs(3);
/// 算得慢的仓库少算几次：两次之间至少隔这么多倍的用时。
const REST_FACTOR: u32 = 5;

#[derive(Debug, Default)]
struct Slot {
    stat: Option<DiffStat>,
    /// 上次算完的时间；还没算过为 None。
    done: Option<Instant>,
    took: Duration,
    running: bool,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct Diffs(Arc<Mutex<HashMap<TaskId, Slot>>>);

impl Diffs {
    fn lock(&self) -> MutexGuard<'_, HashMap<TaskId, Slot>> {
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// 最近一次的结果，过期了就在后台重新算。
    pub(crate) fn get(&self, id: TaskId, worktree: &Path, base: &str) -> Option<DiffStat> {
        let mut slots = self.lock();
        let slot = slots.entry(id).or_default();
        let rest = REFRESH.max(slot.took * REST_FACTOR);
        if !slot.running && slot.done.is_none_or(|at| at.elapsed() >= rest) {
            slot.running = true;
            let (diffs, worktree, base) = (self.clone(), worktree.to_path_buf(), base.to_string());
            thread::spawn(move || {
                let started = Instant::now();
                let stat = duolo_git::diff_stat(&worktree, &base).ok();
                // 算的时候任务可能已经关了，那就不用记了。
                if let Some(slot) = diffs.lock().get_mut(&id) {
                    *slot = Slot { stat, done: Some(Instant::now()), took: started.elapsed(), running: false };
                }
            });
        }
        slot.stat
    }

    /// 第一次还没算完。
    pub(crate) fn pending(&self, id: TaskId) -> bool {
        self.lock().get(&id).is_some_and(|slot| slot.done.is_none() && slot.running)
    }

    pub(crate) fn remove(&self, id: TaskId) {
        self.lock().remove(&id);
    }
}
