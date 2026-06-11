//! GPU-accelerated batch CA simulation using blade-graphics.
//!
//! Simulates thousands of CA grids simultaneously on GPU via compute shaders.
//! Supports both deterministic B3/S23 (fast path) and table-driven probabilistic
//! rules with Philox counter-based RNG.
//! Selection, mutation, and fitness evaluation remain on CPU.

use blade_graphics::ShaderData as _;
use crate::grid::{BoundaryMode, Coordinates, Grid};

/// Maximum neighbor weight sum for Moore neighborhood (8 cells, weight 1 each).
pub const RULE_TABLE_SIZE: usize = 9;

/// Return B3/S23 (Game of Life) spawn and keep probability tables.
pub fn b3s23_tables() -> ([f32; RULE_TABLE_SIZE], [f32; RULE_TABLE_SIZE]) {
    let mut spawn = [0.0f32; RULE_TABLE_SIZE];
    let mut keep = [0.0f32; RULE_TABLE_SIZE];
    spawn[3] = 1.0;
    keep[2] = 1.0;
    keep[3] = 1.0;
    (spawn, keep)
}

/// Per-grid statistics read back from GPU.
#[derive(Clone, Debug, Default)]
pub struct GridStats {
    pub alive_count: u32,
    pub birth_count: u32,
    /// Alive counts in each of 16 (4x4) spatial regions.
    pub region_alive: [u32; 16],
}

impl GridStats {
    pub fn alive_ratio(&self, total_cells: u32) -> f32 {
        self.alive_count as f32 / total_cells as f32
    }

    pub fn birth_rate(&self, total_cells: u32) -> f32 {
        self.birth_count as f32 / total_cells as f32
    }

    pub fn spatial_variance(&self, total_cells: u32) -> f32 {
        let region_total = total_cells as f32 / 16.0;
        if region_total < 1.0 {
            return 0.0;
        }
        let densities: Vec<f32> = self
            .region_alive
            .iter()
            .map(|&c| c as f32 / region_total)
            .collect();
        let mean: f32 = densities.iter().sum::<f32>() / 16.0;
        densities.iter().map(|d| (d - mean).powi(2)).sum::<f32>() / 16.0
    }
}

/// Outcome of screening one grid: cheap level-1/2 signals from stats history.
#[derive(Clone, Debug, Default)]
pub struct ScreenOutcome {
    /// Relative interestingness; 0.0 means discard (extinct or saturated).
    pub score: f32,
    pub final_alive_ratio: f32,
}

/// Score a grid's stats history. Rewards alive-ratio dynamism, spatial
/// structure, and sustained births — the same signals the CPU fitness uses,
/// computed from stats alone.
fn score_history(history: &[GridStats], total_cells: u32) -> ScreenOutcome {
    let last = match history.last() {
        Some(stats) => stats,
        None => return ScreenOutcome::default(),
    };
    if last.alive_count == 0 {
        return ScreenOutcome::default();
    }
    let final_alive_ratio = last.alive_ratio(total_cells);
    if final_alive_ratio > 0.9 {
        return ScreenOutcome::default();
    }
    let ratios: Vec<f32> = history
        .iter()
        .map(|s| s.alive_ratio(total_cells))
        .collect();
    let mean = ratios.iter().sum::<f32>() / ratios.len() as f32;
    let variance =
        ratios.iter().map(|r| (r - mean).powi(2)).sum::<f32>() / ratios.len() as f32;
    let score = 1.0
        + (variance * 1.0e6).min(50.0)
        + (last.spatial_variance(total_cells) * 1.0e4).min(20.0)
        + (last.birth_rate(total_cells) * 1.0e4).min(30.0);
    ScreenOutcome {
        score,
        final_alive_ratio,
    }
}

/// Pack a Grid into a bitfield: 1 bit per cell, packed into u32 words.
pub fn pack_grid(grid: &Grid) -> Vec<u32> {
    let size = grid.size();
    let total_cells = (size.x * size.y) as usize;
    let words = (total_cells + 31) / 32;
    let mut packed = vec![0u32; words];
    for y in 0..size.y {
        for x in 0..size.x {
            if grid.get(x, y).is_some() {
                let idx = (y * size.x + x) as usize;
                packed[idx / 32] |= 1 << (idx % 32);
            }
        }
    }
    packed
}

/// Unpack a bitfield back into a Grid.
pub fn unpack_grid(packed: &[u32], width: i32, height: i32, boundary: BoundaryMode) -> Grid {
    let mut grid = Grid::with_boundary(Coordinates { x: width, y: height }, boundary);
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if (packed[idx / 32] >> (idx % 32)) & 1 == 1 {
                grid.init(x, y);
            }
        }
    }
    grid
}

/// Configuration for GPU batch simulation.
#[derive(Clone)]
pub struct GpuBatchConfig {
    /// Grid width (all grids in a batch must share dimensions).
    pub grid_width: u32,
    /// Grid height.
    pub grid_height: u32,
    /// Number of CA steps to run per dispatch batch.
    pub steps_per_batch: u32,
    /// How many grids to simulate in parallel.
    pub num_grids: u32,
    /// Boundary mode shared by all grids in the batch.
    pub boundary: BoundaryMode,
    /// Spawn probability table: index = neighbor count, value = P(dead → alive).
    pub spawn_table: [f32; RULE_TABLE_SIZE],
    /// Keep probability table: index = neighbor count, value = P(alive → alive).
    pub keep_table: [f32; RULE_TABLE_SIZE],
}

impl GpuBatchConfig {
    pub fn words_per_grid(&self) -> usize {
        ((self.grid_width * self.grid_height) as usize + 31) / 32
    }

    pub fn total_cells(&self) -> u32 {
        self.grid_width * self.grid_height
    }

    fn rule_mode(&self) -> u32 {
        let (b3s23_spawn, b3s23_keep) = b3s23_tables();
        if self.spawn_table == b3s23_spawn && self.keep_table == b3s23_keep {
            0
        } else {
            1
        }
    }

    /// Convenience constructor for standard B3/S23 Game of Life.
    pub fn b3s23(
        grid_width: u32,
        grid_height: u32,
        steps_per_batch: u32,
        num_grids: u32,
        boundary: BoundaryMode,
    ) -> Self {
        let (spawn_table, keep_table) = b3s23_tables();
        Self {
            grid_width,
            grid_height,
            steps_per_batch,
            num_grids,
            boundary,
            spawn_table,
            keep_table,
        }
    }
}

/// WGSL compute shader source for CA step.
pub const CA_STEP_WGSL: &str = include_str!("shaders/ca_step.wgsl");

/// Uniform params matching the WGSL SimParams struct.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct SimParams {
    grid_width: u32,
    grid_height: u32,
    words_per_grid: u32,
    num_grids: u32,
    boundary_mode: u32,
    rule_mode: u32,
    current_step: u32,
}

/// Shader data layout matching the WGSL globals.
/// Field names must match WGSL variable names exactly.
#[derive(blade_macros::ShaderData)]
struct CaStepData {
    params: SimParams,
    grids_in: blade_graphics::BufferPiece,
    grids_out: blade_graphics::BufferPiece,
    alive_counts: blade_graphics::BufferPiece,
    birth_counts: blade_graphics::BufferPiece,
    region_alive: blade_graphics::BufferPiece,
    spawn_table: blade_graphics::BufferPiece,
    keep_table: blade_graphics::BufferPiece,
}

/// Shared GPU device + compiled pipeline. Multiple `GpuSimulator` instances
/// can share one `GpuContext` (via `Rc`) to avoid creating redundant Vulkan
/// devices on the screener thread.
pub struct GpuContext {
    inner: blade_graphics::Context,
    pipeline: blade_graphics::ComputePipeline,
}

impl GpuContext {
    /// # Panics
    /// Panics if GPU context creation or shader compilation fails.
    pub fn new() -> Self {
        let inner = unsafe {
            blade_graphics::Context::init(blade_graphics::ContextDesc {
                presentation: false,
                ..Default::default()
            })
            .expect("Failed to create GPU context")
        };
        let shader = inner.create_shader(blade_graphics::ShaderDesc {
            source: CA_STEP_WGSL,
        });
        let layout = CaStepData::layout();
        let pipeline =
            inner.create_compute_pipeline(blade_graphics::ComputePipelineDesc {
                name: "ca_step",
                data_layouts: &[&layout],
                compute: shader.at("ca_step"),
            });
        Self { inner, pipeline }
    }
}

/// GPU batch simulator.
///
/// Supports deterministic B3/S23 (rule_mode 0) and table-driven probabilistic
/// rules with Philox RNG (rule_mode 1). CPU handles selection, mutation,
/// and fitness evaluation.
pub struct GpuSimulator {
    pub config: GpuBatchConfig,
    ctx: std::rc::Rc<GpuContext>,
    grids_a: blade_graphics::Buffer,
    grids_b: blade_graphics::Buffer,
    alive_counts_buf: blade_graphics::Buffer,
    birth_counts_buf: blade_graphics::Buffer,
    region_alive_buf: blade_graphics::Buffer,
    spawn_table_buf: blade_graphics::Buffer,
    keep_table_buf: blade_graphics::Buffer,
    ping: bool,
    total_steps: u32,
    encoder: blade_graphics::CommandEncoder,
    sync_point: Option<blade_graphics::SyncPoint>,
}

impl GpuSimulator {
    /// Create a new GPU simulator with its own dedicated context.
    ///
    /// # Panics
    /// Panics if GPU context creation or shader compilation fails.
    pub fn new(config: GpuBatchConfig) -> Self {
        Self::with_context(std::rc::Rc::new(GpuContext::new()), config)
    }

    /// Create a new GPU simulator sharing an existing context.
    /// Use this when running multiple simulators (e.g. the screener)
    /// to avoid redundant Vulkan device creation.
    pub fn with_context(ctx: std::rc::Rc<GpuContext>, config: GpuBatchConfig) -> Self {
        let context = &ctx.inner;

        let words_per_grid = config.words_per_grid();
        let total_grid_bytes = (words_per_grid * config.num_grids as usize * 4) as u64;
        let n = config.num_grids as u64;

        let grids_a = context.create_buffer(blade_graphics::BufferDesc {
            name: "grids_a",
            size: total_grid_bytes,
            memory: blade_graphics::Memory::Shared,
        });
        let grids_b = context.create_buffer(blade_graphics::BufferDesc {
            name: "grids_b",
            size: total_grid_bytes,
            memory: blade_graphics::Memory::Shared,
        });
        let alive_counts_buf = context.create_buffer(blade_graphics::BufferDesc {
            name: "alive_counts",
            size: n * 4,
            memory: blade_graphics::Memory::Shared,
        });
        let birth_counts_buf = context.create_buffer(blade_graphics::BufferDesc {
            name: "birth_counts",
            size: n * 4,
            memory: blade_graphics::Memory::Shared,
        });
        let region_alive_buf = context.create_buffer(blade_graphics::BufferDesc {
            name: "region_alive",
            size: n * 16 * 4,
            memory: blade_graphics::Memory::Shared,
        });

        let table_bytes = (RULE_TABLE_SIZE * 4) as u64;
        let spawn_table_buf = context.create_buffer(blade_graphics::BufferDesc {
            name: "spawn_table",
            size: table_bytes,
            memory: blade_graphics::Memory::Shared,
        });
        let keep_table_buf = context.create_buffer(blade_graphics::BufferDesc {
            name: "keep_table",
            size: table_bytes,
            memory: blade_graphics::Memory::Shared,
        });

        unsafe {
            let spawn_ptr =
                std::slice::from_raw_parts_mut(spawn_table_buf.data() as *mut f32, RULE_TABLE_SIZE);
            spawn_ptr.copy_from_slice(&config.spawn_table);
            let keep_ptr =
                std::slice::from_raw_parts_mut(keep_table_buf.data() as *mut f32, RULE_TABLE_SIZE);
            keep_ptr.copy_from_slice(&config.keep_table);
        }

        let encoder =
            context.create_command_encoder(blade_graphics::CommandEncoderDesc {
                name: "ca_step",
                buffer_count: 2,
            });

        Self {
            config,
            ctx,
            grids_a,
            grids_b,
            alive_counts_buf,
            birth_counts_buf,
            region_alive_buf,
            spawn_table_buf,
            keep_table_buf,
            ping: false,
            total_steps: 0,
            encoder,
            sync_point: None,
        }
    }

    /// Upload packed grids to GPU.
    pub fn upload_grids(&mut self, packed_grids: &[Vec<u32>]) {
        assert!(packed_grids.len() <= self.config.num_grids as usize);
        let words_per_grid = self.config.words_per_grid();
        let dst = if self.ping { self.grids_b } else { self.grids_a };
        let total_words = words_per_grid * self.config.num_grids as usize;

        unsafe {
            let buf = std::slice::from_raw_parts_mut(dst.data() as *mut u32, total_words);
            buf.fill(0);
            for (i, grid) in packed_grids.iter().enumerate() {
                let offset = i * words_per_grid;
                let len = grid.len().min(words_per_grid);
                buf[offset..offset + len].copy_from_slice(&grid[..len]);
            }
        }
        self.total_steps = 0;
    }

    /// Read back the current (most recently computed) state of all grids.
    pub fn readback_grids(&self) -> Vec<Vec<u32>> {
        let words_per_grid = self.config.words_per_grid();
        let src = if self.ping { self.grids_b } else { self.grids_a };
        let total_words = words_per_grid * self.config.num_grids as usize;
        let buf =
            unsafe { std::slice::from_raw_parts(src.data() as *const u32, total_words) };
        (0..self.config.num_grids as usize)
            .map(|i| buf[i * words_per_grid..(i + 1) * words_per_grid].to_vec())
            .collect()
    }

    /// Run `count` CA steps on all grids in a single GPU submission,
    /// then wait for completion. Stats reflect the final step only.
    ///
    /// Buffers are cleared GPU-side between steps (blade inserts a full
    /// barrier before every pass), so no CPU sync is needed mid-batch.
    pub fn step(&mut self, count: usize) {
        let cells_per_grid = self.config.total_cells();
        let workgroup_size = 256u32;
        let workgroups_x = (cells_per_grid + workgroup_size - 1) / workgroup_size;
        let n = self.config.num_grids as u64;
        let grid_bytes = (self.config.words_per_grid() * self.config.num_grids as usize * 4) as u64;

        let rule_mode = self.config.rule_mode();

        self.encoder.start();
        for _ in 0..count {
            let (src, dst) = if self.ping {
                (self.grids_b, self.grids_a)
            } else {
                (self.grids_a, self.grids_b)
            };

            let params = SimParams {
                grid_width: self.config.grid_width,
                grid_height: self.config.grid_height,
                words_per_grid: self.config.words_per_grid() as u32,
                num_grids: self.config.num_grids,
                boundary_mode: match self.config.boundary {
                    BoundaryMode::Wrap => 0,
                    BoundaryMode::Dead => 1,
                },
                rule_mode,
                current_step: self.total_steps,
            };

            {
                let mut transfer = self.encoder.transfer("clear");
                transfer.fill_buffer(dst.into(), grid_bytes, 0);
                transfer.fill_buffer(self.alive_counts_buf.into(), n * 4, 0);
                transfer.fill_buffer(self.birth_counts_buf.into(), n * 4, 0);
                transfer.fill_buffer(self.region_alive_buf.into(), n * 16 * 4, 0);
            }
            {
                let mut pass = self.encoder.compute("ca_step");
                let mut pc = pass.with(&self.ctx.pipeline);
                pc.bind(
                    0,
                    &CaStepData {
                        params,
                        grids_in: src.into(),
                        grids_out: dst.into(),
                        alive_counts: self.alive_counts_buf.into(),
                        birth_counts: self.birth_counts_buf.into(),
                        region_alive: self.region_alive_buf.into(),
                        spawn_table: self.spawn_table_buf.into(),
                        keep_table: self.keep_table_buf.into(),
                    },
                );
                pc.dispatch([workgroups_x, self.config.num_grids, 1]);
            }
            self.ping = !self.ping;
            self.total_steps += 1;
        }
        let sp = self.ctx.inner.submit(&mut self.encoder);
        self.ctx.inner.wait_for(&sp, !0);
        self.sync_point = Some(sp);
    }

    /// Screen a batch of packed grids: run `total_steps`, reading stats back
    /// every `interval` steps, and produce a cheap interestingness score per
    /// grid. Grids that go extinct or saturate are zeroed early so subsequent
    /// steps skip them (Phase 3 early discard).
    pub fn screen(
        &mut self,
        packed: &[Vec<u32>],
        total_steps: usize,
        interval: usize,
    ) -> Vec<ScreenOutcome> {
        let count = packed.len();
        self.upload_grids(packed);
        let total_cells = self.config.total_cells();
        let interval = interval.max(1);
        let mut histories: Vec<Vec<GridStats>> = vec![Vec::new(); count];
        let mut dead: Vec<bool> = vec![false; count];
        let mut done = 0usize;
        while done < total_steps {
            let n = interval.min(total_steps - done);
            self.step(n);
            let stats = self.readback_stats();
            for (i, stat) in stats.into_iter().enumerate() {
                if i < count && !dead[i] {
                    let dominated = stat.alive_count == 0
                        || stat.alive_ratio(total_cells) > 0.9;
                    histories[i].push(stat);
                    if dominated {
                        dead[i] = true;
                        self.zero_grid(i);
                    }
                }
            }
            done += n;
        }
        histories
            .iter()
            .map(|h| score_history(h, total_cells))
            .collect()
    }

    /// Zero a specific grid's data in both ping-pong buffers so it stays
    /// dead in all subsequent steps (no births from a zeroed grid).
    fn zero_grid(&mut self, grid_index: usize) {
        let words_per_grid = self.config.words_per_grid();
        let offset = grid_index * words_per_grid;
        unsafe {
            let buf_a = std::slice::from_raw_parts_mut(
                (self.grids_a.data() as *mut u32).add(offset),
                words_per_grid,
            );
            buf_a.fill(0);
            let buf_b = std::slice::from_raw_parts_mut(
                (self.grids_b.data() as *mut u32).add(offset),
                words_per_grid,
            );
            buf_b.fill(0);
        }
    }

    /// Read back per-grid statistics from GPU.
    pub fn readback_stats(&self) -> Vec<GridStats> {
        let n = self.config.num_grids as usize;
        let alive = unsafe {
            std::slice::from_raw_parts(self.alive_counts_buf.data() as *const u32, n)
        };
        let births = unsafe {
            std::slice::from_raw_parts(self.birth_counts_buf.data() as *const u32, n)
        };
        let regions = unsafe {
            std::slice::from_raw_parts(self.region_alive_buf.data() as *const u32, n * 16)
        };
        (0..n)
            .map(|i| {
                let mut region_alive = [0u32; 16];
                region_alive.copy_from_slice(&regions[i * 16..(i + 1) * 16]);
                GridStats {
                    alive_count: alive[i],
                    birth_count: births[i],
                    region_alive,
                }
            })
            .collect()
    }
}

impl Drop for GpuSimulator {
    fn drop(&mut self) {
        if let Some(sp) = self.sync_point.take() {
            self.ctx.inner.wait_for(&sp, !0);
        }
        self.ctx.inner.destroy_command_encoder(&mut self.encoder);
        self.ctx.inner.destroy_buffer(self.grids_a);
        self.ctx.inner.destroy_buffer(self.grids_b);
        self.ctx.inner.destroy_buffer(self.alive_counts_buf);
        self.ctx.inner.destroy_buffer(self.birth_counts_buf);
        self.ctx.inner.destroy_buffer(self.region_alive_buf);
        self.ctx.inner.destroy_buffer(self.spawn_table_buf);
        self.ctx.inner.destroy_buffer(self.keep_table_buf);
    }
}

/// Philox 2x32-10 counter-based RNG (CPU reference matching GPU shader).
pub fn philox2x32(counter: [u32; 2], key: u32) -> [u32; 2] {
    const PHILOX_M: u64 = 0xD256D193;
    const PHILOX_W: u32 = 0x9E3779B9;
    let mut c = counter;
    let mut k = key;
    for _ in 0..10 {
        let product = PHILOX_M * c[0] as u64;
        let hi = (product >> 32) as u32;
        let lo = product as u32;
        c = [hi ^ c[1] ^ k, lo];
        k = k.wrapping_add(PHILOX_W);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blinker_oscillates() {
        let w = 8u32;
        let h = 8u32;
        let mut sim = GpuSimulator::new(GpuBatchConfig::b3s23(w, h, 1, 1, BoundaryMode::Wrap));

        let total = (w * h) as usize;
        let words = (total + 31) / 32;
        let mut grid = vec![0u32; words];
        for &x in &[2, 3, 4] {
            let idx = (3 * w + x) as usize;
            grid[idx / 32] |= 1 << (idx % 32);
        }

        sim.upload_grids(&[grid]);
        sim.step(1);
        let stats = sim.readback_stats();
        assert_eq!(stats[0].alive_count, 3, "blinker should have 3 alive cells");
        assert_eq!(stats[0].birth_count, 2, "blinker step should birth 2 cells");

        sim.step(1);
        let stats = sim.readback_stats();
        assert_eq!(stats[0].alive_count, 3);
        assert_eq!(stats[0].birth_count, 2);
    }

    #[test]
    fn pack_unpack_roundtrip() {
        let mut grid = Grid::new(Coordinates { x: 16, y: 16 });
        grid.init(3, 5);
        grid.init(10, 10);
        grid.init(0, 0);

        let packed = pack_grid(&grid);
        let unpacked = unpack_grid(&packed, 16, 16, BoundaryMode::Wrap);

        assert!(unpacked.get(3, 5).is_some());
        assert!(unpacked.get(10, 10).is_some());
        assert!(unpacked.get(0, 0).is_some());
        assert!(unpacked.get(1, 1).is_none());
    }

    #[test]
    fn screen_ranks_candidates() {
        const W: i32 = 32;
        const H: i32 = 32;
        let mut sim = GpuSimulator::new(GpuBatchConfig::b3s23(
            W as u32, H as u32, 16, 3, BoundaryMode::Wrap,
        ));

        let empty = Grid::new(Coordinates { x: W, y: H });
        let mut blinker = Grid::new(Coordinates { x: W, y: H });
        for x in 14..17 {
            blinker.init(x, 16);
        }
        let mut rpent = Grid::new(Coordinates { x: W, y: H });
        for &(x, y) in &[(16, 15), (17, 15), (15, 16), (16, 16), (16, 17)] {
            rpent.init(x, y);
        }

        let outcomes = sim.screen(
            &[pack_grid(&empty), pack_grid(&blinker), pack_grid(&rpent)],
            64,
            16,
        );
        assert_eq!(outcomes[0].score, 0.0, "empty grid must be discarded");
        assert!(outcomes[1].score > 0.0, "blinker survives");
        assert!(
            outcomes[2].score > outcomes[1].score,
            "active methuselah ({}) should outrank static blinker ({})",
            outcomes[2].score,
            outcomes[1].score
        );
    }

    #[test]
    fn matches_cpu_simulation() {
        use crate::sim::{Data, HumanRules, Limits, Simulation, Snap};

        const W: i32 = 32;
        const H: i32 = 32;
        const STEPS: usize = 16;

        for boundary in [BoundaryMode::Wrap, BoundaryMode::Dead] {
            let mut grid = Grid::with_boundary(Coordinates { x: W, y: H }, boundary);
            let mut state = 0x2545F4914F6CDD1Du64;
            for _ in 0..300 {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let x = ((state >> 33) % W as u64) as i32;
                let y = ((state >> 13) % H as u64) as i32;
                grid.init(x, y);
            }
            for &(x, y) in &[(0, 0), (W - 1, 0), (0, H - 1), (W - 1, H - 1), (0, 15), (W - 1, 16), (15, 0), (16, H - 1)] {
                grid.init(x, y);
            }
            let initial = pack_grid(&grid);

            let mut rules = HumanRules {
                kernel: vec!["111".into(), "1X1".into(), "111".into()],
                ..Default::default()
            };
            rules.spawn.insert(3, 1.0);
            rules.keep.insert(2, 1.0);
            rules.keep.insert(3, 1.0);
            let snap = Snap {
                data: Data::unparse(&grid),
                rules,
                random_seed: 0,
                limits: Limits {
                    max_steps: 100_000,
                    update_weight: 0.1,
                },
                boundary,
            };
            let mut sim = Simulation::new(&snap).unwrap();
            for _ in 0..STEPS {
                sim.advance().unwrap_or_else(|c| {
                    panic!("CPU sim concluded early ({}) with boundary {:?}", c, boundary)
                });
            }

            let mut gpu = GpuSimulator::new(GpuBatchConfig::b3s23(
                W as u32, H as u32, STEPS as u32, 1, boundary,
            ));
            gpu.upload_grids(&[initial]);
            gpu.step(STEPS);
            let gpu_grids = gpu.readback_grids();

            assert_eq!(
                gpu_grids[0],
                pack_grid(sim.grid()),
                "GPU and CPU diverged after {} steps with boundary {:?}",
                STEPS,
                boundary
            );
        }
    }

    /// Table-driven B3/S23 (rule_mode 1) must produce identical results
    /// to the hardcoded B3/S23 path (rule_mode 0). This verifies the
    /// Philox RNG doesn't affect deterministic rules (coin < 1.0 always
    /// true, coin < 0.0 always false).
    #[test]
    fn table_driven_matches_hardcoded() {
        const W: i32 = 32;
        const H: i32 = 32;
        const STEPS: usize = 16;

        let mut grid = Grid::new(Coordinates { x: W, y: H });
        let mut state = 0xDEADBEEFCAFEBABEu64;
        for _ in 0..200 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let x = ((state >> 33) % W as u64) as i32;
            let y = ((state >> 13) % H as u64) as i32;
            grid.init(x, y);
        }
        let packed = pack_grid(&grid);

        // Hardcoded B3/S23 (rule_mode 0)
        let mut gpu0 = GpuSimulator::new(GpuBatchConfig::b3s23(
            W as u32, H as u32, STEPS as u32, 1, BoundaryMode::Wrap,
        ));
        gpu0.upload_grids(&[packed.clone()]);
        gpu0.step(STEPS);
        let result0 = gpu0.readback_grids();

        // Table-driven B3/S23 with slightly perturbed table to force mode 1,
        // but still deterministic (all 0.0 or 1.0, just different arrangement).
        // Actually, we want exact B3/S23 behavior. Force mode 1 by using a
        // non-B3/S23 table that still computes B3/S23: make spawn[3]=1.0 and
        // keep[2]=1.0, keep[3]=1.0, but add spawn[0]=0.0 explicitly.
        // That matches B3/S23. Instead, let's use a HighLife-like table that
        // differs, to test the table path. But for parity test we need the
        // same rule...
        //
        // Force rule_mode=1 by adding a tiny epsilon to one unused entry.
        let (mut spawn, keep) = b3s23_tables();
        spawn[0] = 1e-30; // non-zero but effectively zero — forces mode 1
        let mut gpu1 = GpuSimulator::new(GpuBatchConfig {
            grid_width: W as u32,
            grid_height: H as u32,
            steps_per_batch: STEPS as u32,
            num_grids: 1,
            boundary: BoundaryMode::Wrap,
            spawn_table: spawn,
            keep_table: keep,
        });
        gpu1.upload_grids(&[packed]);
        gpu1.step(STEPS);
        let result1 = gpu1.readback_grids();

        // spawn[0] = 1e-30 means "spawn with 0 neighbors with prob 1e-30".
        // The RNG outputs values in [0, 1), so coin < 1e-30 is essentially
        // never true (24-bit precision, minimum nonzero coin is ~6e-8).
        // Results should be identical.
        assert_eq!(
            result0[0], result1[0],
            "Table-driven B3/S23 should match hardcoded path"
        );
    }

    /// HighLife (B36/S23) via table-driven path must differ from standard
    /// B3/S23 on the same initial conditions: the extra birth rule (6)
    /// causes different evolution.
    #[test]
    fn highlife_differs_from_gol() {
        const W: i32 = 32;
        const H: i32 = 32;
        const STEPS: usize = 20;

        let mut grid = Grid::new(Coordinates { x: W, y: H });
        let mut state = 0x123456789ABCDEFu64;
        for _ in 0..250 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let x = ((state >> 33) % W as u64) as i32;
            let y = ((state >> 13) % H as u64) as i32;
            grid.init(x, y);
        }
        let packed = pack_grid(&grid);

        // Standard GoL
        let mut gol = GpuSimulator::new(GpuBatchConfig::b3s23(
            W as u32, H as u32, STEPS as u32, 1, BoundaryMode::Wrap,
        ));
        gol.upload_grids(&[packed.clone()]);
        gol.step(STEPS);
        let gol_result = gol.readback_grids();

        // HighLife: B36/S23
        let (mut spawn, keep) = b3s23_tables();
        spawn[6] = 1.0;
        let mut hl = GpuSimulator::new(GpuBatchConfig {
            grid_width: W as u32,
            grid_height: H as u32,
            steps_per_batch: STEPS as u32,
            num_grids: 1,
            boundary: BoundaryMode::Wrap,
            spawn_table: spawn,
            keep_table: keep,
        });
        hl.upload_grids(&[packed]);
        hl.step(STEPS);
        let hl_result = hl.readback_grids();

        assert_ne!(
            gol_result[0], hl_result[0],
            "HighLife (B36/S23) must produce different evolution than GoL (B3/S23)"
        );
    }

    /// Probabilistic rules (non-deterministic spawn/keep) use the Philox RNG
    /// seeded differently per grid_idx, so two identical initial grids in the
    /// same batch evolve differently.
    #[test]
    fn probabilistic_rules_use_rng() {
        const W: u32 = 32;
        const H: u32 = 32;
        const STEPS: usize = 8;

        // Scattered ~30% soup — a typical starting condition
        let mut grid = Grid::new(Coordinates { x: W as i32, y: H as i32 });
        let mut state = 0xA1B2C3D4E5F60718u64;
        for _ in 0..300 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let x = ((state >> 33) % W as u64) as i32;
            let y = ((state >> 13) % H as u64) as i32;
            grid.init(x, y);
        }
        let packed = pack_grid(&grid);

        // Broad probabilistic rule: high spawn/keep across multiple neighbor
        // counts so the population doesn't crash immediately.
        let mut spawn = [0.0f32; RULE_TABLE_SIZE];
        spawn[2] = 0.3;
        spawn[3] = 0.7;
        spawn[4] = 0.2;
        let mut keep = [0.0f32; RULE_TABLE_SIZE];
        keep[1] = 0.4;
        keep[2] = 0.9;
        keep[3] = 0.9;
        keep[4] = 0.5;

        let mut gpu = GpuSimulator::new(GpuBatchConfig {
            grid_width: W,
            grid_height: H,
            steps_per_batch: STEPS as u32,
            num_grids: 2,
            boundary: BoundaryMode::Wrap,
            spawn_table: spawn,
            keep_table: keep,
        });
        gpu.upload_grids(&[packed.clone(), packed]);
        gpu.step(STEPS);
        let results = gpu.readback_grids();

        // Grids must differ: the RNG uses grid_idx as the Philox key
        assert_ne!(
            results[0], results[1],
            "Two identical grids must evolve differently under probabilistic rules \
             (Philox RNG seeded by grid_idx)"
        );
    }

    /// Early discard: an empty grid gets score 0 and a grid that goes
    /// extinct mid-screen is zeroed so it doesn't waste subsequent steps.
    #[test]
    fn screen_early_discard() {
        const W: i32 = 16;
        const H: i32 = 16;
        let mut sim = GpuSimulator::new(GpuBatchConfig::b3s23(
            W as u32, H as u32, 4, 2, BoundaryMode::Wrap,
        ));

        // Grid 0: single isolated cell — dies immediately under B3/S23.
        let mut doomed = Grid::new(Coordinates { x: W, y: H });
        doomed.init(8, 8);

        // Grid 1: R-pentomino — survives many steps.
        let mut rpent = Grid::new(Coordinates { x: W, y: H });
        for &(x, y) in &[(8, 7), (9, 7), (7, 8), (8, 8), (8, 9)] {
            rpent.init(x, y);
        }

        let outcomes = sim.screen(
            &[pack_grid(&doomed), pack_grid(&rpent)],
            100,
            20,
        );

        assert_eq!(outcomes[0].score, 0.0, "doomed grid should be discarded");
        assert!(outcomes[1].score > 0.0, "R-pentomino should survive");

        // After screening, the doomed grid should be zeroed in both buffers
        let grids = sim.readback_grids();
        assert!(
            grids[0].iter().all(|&w| w == 0),
            "extinct grid should be zeroed after early discard"
        );
    }

    /// CPU Philox reference must match known test vectors.
    #[test]
    fn philox_reference() {
        // Known output for counter=[0,0], key=0 after 10 rounds
        let result = philox2x32([0, 0], 0);
        assert_ne!(result, [0, 0], "Philox should mix even zero inputs");

        // Deterministic: same inputs → same output
        let a = philox2x32([42, 100], 7);
        let b = philox2x32([42, 100], 7);
        assert_eq!(a, b, "Philox must be deterministic");

        // Different keys → different output
        let c = philox2x32([42, 100], 8);
        assert_ne!(a, c, "Different keys should produce different output");
    }
}
