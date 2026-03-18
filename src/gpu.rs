//! GPU-accelerated batch CA simulation using blade-graphics.
//!
//! Simulates thousands of CA grids simultaneously on GPU via compute shaders.
//! Selection, mutation, and fitness evaluation remain on CPU.

use blade_graphics::ShaderData as _;
use crate::grid::{BoundaryMode, Coordinates, Grid};

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
#[derive(Clone, Copy)]
pub struct GpuBatchConfig {
    /// Grid width (all grids in a batch must share dimensions).
    pub grid_width: u32,
    /// Grid height.
    pub grid_height: u32,
    /// Number of CA steps to run per dispatch batch.
    pub steps_per_batch: u32,
    /// How many grids to simulate in parallel.
    pub num_grids: u32,
}

impl GpuBatchConfig {
    pub fn words_per_grid(&self) -> usize {
        ((self.grid_width * self.grid_height) as usize + 31) / 32
    }

    pub fn total_cells(&self) -> u32 {
        self.grid_width * self.grid_height
    }
}

/// WGSL compute shader source for CA step (B3/S23 deterministic).
pub const CA_STEP_WGSL: &str = include_str!("shaders/ca_step.wgsl");

/// Uniform params matching the WGSL SimParams struct.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
struct SimParams {
    grid_width: u32,
    grid_height: u32,
    words_per_grid: u32,
    num_grids: u32,
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
}

/// GPU batch simulator.
///
/// Phase 1: deterministic B3/S23 (Game of Life) on GPU.
/// CPU handles selection, mutation, fitness evaluation.
pub struct GpuSimulator {
    pub config: GpuBatchConfig,
    context: blade_graphics::Context,
    grids_a: blade_graphics::Buffer,
    grids_b: blade_graphics::Buffer,
    alive_counts_buf: blade_graphics::Buffer,
    birth_counts_buf: blade_graphics::Buffer,
    region_alive_buf: blade_graphics::Buffer,
    pipeline: blade_graphics::ComputePipeline,
    ping: bool,
    encoder: blade_graphics::CommandEncoder,
    sync_point: Option<blade_graphics::SyncPoint>,
}

impl GpuSimulator {
    /// Create a new GPU simulator.
    ///
    /// # Panics
    /// Panics if GPU context creation or shader compilation fails.
    pub fn new(config: GpuBatchConfig) -> Self {
        let context = unsafe {
            blade_graphics::Context::init(blade_graphics::ContextDesc {
                presentation: false,
                ..Default::default()
            })
            .expect("Failed to create GPU context")
        };

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

        let shader = context.create_shader(blade_graphics::ShaderDesc {
            source: CA_STEP_WGSL,
        });
        let layout = CaStepData::layout();
        let pipeline =
            context.create_compute_pipeline(blade_graphics::ComputePipelineDesc {
                name: "ca_step",
                data_layouts: &[&layout],
                compute: shader.at("ca_step"),
            });

        let encoder =
            context.create_command_encoder(blade_graphics::CommandEncoderDesc {
                name: "ca_step",
                buffer_count: 2,
            });

        Self {
            config,
            context,
            grids_a,
            grids_b,
            alive_counts_buf,
            birth_counts_buf,
            region_alive_buf,
            pipeline,
            ping: false,
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
    }

    fn zero_stats(&self) {
        let n = self.config.num_grids as usize;
        unsafe {
            std::slice::from_raw_parts_mut(self.alive_counts_buf.data() as *mut u32, n).fill(0);
            std::slice::from_raw_parts_mut(self.birth_counts_buf.data() as *mut u32, n).fill(0);
            std::slice::from_raw_parts_mut(self.region_alive_buf.data() as *mut u32, n * 16)
                .fill(0);
        }
    }

    fn zero_grid(&self, buf: blade_graphics::Buffer) {
        let total_words = self.config.words_per_grid() * self.config.num_grids as usize;
        unsafe {
            std::slice::from_raw_parts_mut(buf.data() as *mut u32, total_words).fill(0);
        }
    }

    /// Run `count` CA steps on all grids, then wait for completion.
    pub fn step(&mut self, count: usize) {
        let cells_per_grid = self.config.total_cells();
        let workgroup_size = 256u32;
        let workgroups_x = (cells_per_grid + workgroup_size - 1) / workgroup_size;

        let params = SimParams {
            grid_width: self.config.grid_width,
            grid_height: self.config.grid_height,
            words_per_grid: self.config.words_per_grid() as u32,
            num_grids: self.config.num_grids,
        };

        for _ in 0..count {
            let (src, dst) = if self.ping {
                (self.grids_b, self.grids_a)
            } else {
                (self.grids_a, self.grids_b)
            };

            // Zero destination grid and stats
            self.zero_grid(dst);
            self.zero_stats();

            self.encoder.start();
            {
                let mut pass = self.encoder.compute("ca_step");
                let mut pc = pass.with(&self.pipeline);
                pc.bind(
                    0,
                    &CaStepData {
                        params,
                        grids_in: src.into(),
                        grids_out: dst.into(),
                        alive_counts: self.alive_counts_buf.into(),
                        birth_counts: self.birth_counts_buf.into(),
                        region_alive: self.region_alive_buf.into(),
                    },
                );
                pc.dispatch([workgroups_x, self.config.num_grids, 1]);
            }
            let sp = self.context.submit(&mut self.encoder);
            self.context.wait_for(&sp, !0);
            self.sync_point = Some(sp);
            self.ping = !self.ping;
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
            self.context.wait_for(&sp, !0);
        }
        self.context.destroy_command_encoder(&mut self.encoder);
        self.context.destroy_buffer(self.grids_a);
        self.context.destroy_buffer(self.grids_b);
        self.context.destroy_buffer(self.alive_counts_buf);
        self.context.destroy_buffer(self.birth_counts_buf);
        self.context.destroy_buffer(self.region_alive_buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blinker_oscillates() {
        // 8x8 grid with a horizontal blinker at row 3, cols 2-4.
        let w = 8u32;
        let h = 8u32;
        let mut sim = GpuSimulator::new(GpuBatchConfig {
            grid_width: w,
            grid_height: h,
            steps_per_batch: 1,
            num_grids: 1,
        });

        // Pack a blinker: cells (2,3), (3,3), (4,3).
        let total = (w * h) as usize;
        let words = (total + 31) / 32;
        let mut grid = vec![0u32; words];
        for &x in &[2, 3, 4] {
            let idx = (3 * w + x) as usize;
            grid[idx / 32] |= 1 << (idx % 32);
        }

        // Step 1: horizontal → vertical
        sim.upload_grids(&[grid]);
        sim.step(1);
        let stats = sim.readback_stats();
        assert_eq!(stats[0].alive_count, 3, "blinker should have 3 alive cells");
        assert_eq!(stats[0].birth_count, 2, "blinker step should birth 2 cells");

        // Step 2: vertical → horizontal (back to original)
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
}
