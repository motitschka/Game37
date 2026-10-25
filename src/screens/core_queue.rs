//! The Core download queue (FR-EMB-2) that Settings → Cores and First-run Setup's Cores step share:
//! a fixed list of Cores, each with its download state. One download runs at a time and the rest
//! wait in order; PPSSPP downloads its assets first, then the Core, on one bar, as `DownloadCore`
//! does. Dropping the queue drops the running `Download`, which cancels it and removes its temp
//! files.

use std::collections::VecDeque;
use std::path::Path;

use crate::cores::{self, CoreInfo, Download, DownloadState};
use crate::ctx::Ctx;
use crate::ui::Rect;
use crate::ui::gfx::Gfx;
use crate::ui::theme::Colour;

/// Where a Core is in its download, beyond installed or not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Job {
    Idle,
    Waiting,
    /// How far the whole job has got, from 0 to 1, when the size is known.
    Running(Option<f32>),
    Failed,
    Done,
}

/// The download running now: for which Core, and whether this step is PPSSPP's assets.
struct Current {
    core: usize,
    assets: bool,
    download: Download,
}

pub struct CoreQueue {
    cores: Vec<&'static CoreInfo>,
    jobs: Vec<Job>,
    /// Whether each Core is installed, as of the last check: since `recheck`, or a download ended.
    installed: Option<Vec<bool>>,
    /// Cores waiting their turn, in order.
    waiting: VecDeque<usize>,
    current: Option<Current>,
}

impl CoreQueue {
    pub fn new(cores: Vec<&'static CoreInfo>) -> CoreQueue {
        CoreQueue {
            jobs: vec![Job::Idle; cores.len()],
            cores,
            installed: None,
            waiting: VecDeque::new(),
            current: None,
        }
    }

    pub fn job(&self, i: usize) -> Job {
        self.jobs[i]
    }

    /// Whether each Core is installed under `data_dir`, checked once until `recheck`.
    pub fn installed(&mut self, data_dir: &Path) -> &[bool] {
        let cores = &self.cores;
        self.installed.get_or_insert_with(|| {
            cores
                .iter()
                .map(|&core| cores::is_installed(data_dir, core))
                .collect()
        })
    }

    /// Forgets which Cores are installed, so the next `installed` looks again.
    pub fn recheck(&mut self) {
        self.installed = None;
    }

    /// Whether Core `i` is waiting or downloading.
    pub fn busy(&self, i: usize) -> bool {
        matches!(self.jobs[i], Job::Waiting | Job::Running(_))
    }

    /// Whether any Core is waiting or downloading.
    pub fn running(&self) -> bool {
        self.current.is_some() || !self.waiting.is_empty()
    }

    /// Queues Core `i` unless it is already on its way; `advance` starts it.
    pub fn enqueue(&mut self, i: usize) {
        if !self.busy(i) {
            self.jobs[i] = Job::Waiting;
            self.waiting.push_back(i);
        }
    }

    /// Starts the next queued download if none is running.
    pub fn advance(&mut self, ctx: &Ctx) {
        if self.current.is_some() {
            return;
        }
        if let Some(i) = self.waiting.pop_front() {
            let core = self.cores[i];
            self.jobs[i] = Job::Running(None);
            self.current = Some(if core.system_in_data_dir {
                Current {
                    core: i,
                    assets: true,
                    download: (ctx.start_assets_download)(ctx.data_dir.clone()),
                }
            } else {
                Current {
                    core: i,
                    assets: false,
                    download: (ctx.start_download)(core, ctx.data_dir.clone()),
                }
            });
        }
    }

    /// Ends the running download with `job` and starts the next.
    fn finish(&mut self, job: Job, ctx: &Ctx) {
        if let Some(current) = self.current.take() {
            self.jobs[current.core] = job;
        }
        self.installed = None;
        self.advance(ctx);
    }

    /// Moves the running download on: its progress, PPSSPP's Core after its assets, or the next
    /// Core once it ends.
    pub fn tick(&mut self, ctx: &Ctx) {
        let Some(current) = &mut self.current else {
            return;
        };
        let two_steps = self.cores[current.core].system_in_data_dir;
        let (step, steps) = match (two_steps, current.assets) {
            (true, true) => (0, 2),
            (true, false) => (1, 2),
            (false, _) => (0, 1),
        };
        match current.download.poll() {
            state @ DownloadState::Running { .. } => {
                self.jobs[current.core] = Job::Running(fraction(&state, step, steps));
            }
            DownloadState::Done(_) if current.assets => {
                let core = self.cores[current.core];
                current.assets = false;
                current.download = (ctx.start_download)(core, ctx.data_dir.clone());
                self.jobs[current.core] = Job::Running(Some(0.5));
            }
            DownloadState::Done(_) => self.finish(Job::Done, ctx),
            DownloadState::Failed(_) => self.finish(Job::Failed, ctx),
        }
    }
}

/// How far a job of `steps` downloads has got, with `step` of them done and the current one at
/// `state`: each step is an equal share of the bar.
fn fraction(state: &DownloadState, step: usize, steps: usize) -> Option<f32> {
    match state {
        DownloadState::Running {
            received,
            total: Some(total),
        } if *total > 0 => {
            let part = (*received as f32 / *total as f32).min(1.0);
            Some((step as f32 + part) / steps as f32)
        }
        _ if step > 0 => Some(step as f32 / steps as f32),
        _ => None,
    }
}

/// A Core's state as Settings words it.
pub fn status(job: Job, installed: bool) -> String {
    match job {
        Job::Idle if installed => "Installed".to_owned(),
        Job::Idle => "Not downloaded".to_owned(),
        Job::Waiting => "Waiting".to_owned(),
        Job::Running(Some(f)) => format!("Downloading {}%", percent(f)),
        Job::Running(None) => "Downloading".to_owned(),
        Job::Failed => "Download failed · Retry".to_owned(),
        Job::Done => "Downloaded".to_owned(),
    }
}

/// A fraction from 0 to 1 as a whole percentage, rounded down.
pub fn percent(f: f32) -> u32 {
    (f * 100.0) as u32
}

/// A progress bar in `r` on a `track`, filled to `f` (0 to 1) with `fill`.
pub fn progress_bar(gfx: &mut Gfx<'_>, r: Rect, f: f32, track: Colour, fill: Colour) {
    let radius = r.h / 2.0;
    gfx.fill_round_rect(r, radius, track);
    let w = r.w * f.clamp(0.0, 1.0);
    if w > 0.0 {
        gfx.fill_round_rect(Rect::new(r.x, r.y, w, r.h), (w / 2.0).min(radius), fill);
    }
}
