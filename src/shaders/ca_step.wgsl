// Cellular automaton step shader — table-driven with optional Philox RNG.
//
// Each invocation processes one cell in one grid.
// global_invocation_id.x = cell index (linear), .y = grid index.
//
// rule_mode 0: hardcoded B3/S23 (no RNG, fastest path)
// rule_mode 1: table-driven probabilistic (Philox counter-based RNG)
//
// NOTE: No @group/@binding decorators — blade assigns them automatically
// by matching these variable names to the Rust ShaderData struct fields.

struct SimParams {
    grid_width: u32,
    grid_height: u32,
    words_per_grid: u32,
    num_grids: u32,
    // 0 = toroidal wrap, 1 = dead boundary (out-of-bounds cells are empty)
    boundary_mode: u32,
    // 0 = hardcoded B3/S23, 1 = table-driven probabilistic
    rule_mode: u32,
    // Global step counter (used as RNG seed component)
    current_step: u32,
}

var<uniform> params: SimParams;
var<storage, read> grids_in: array<u32>;
var<storage, read_write> grids_out: array<atomic<u32>>;
// Flat stats arrays: one entry per grid.
// (WGSL doesn't allow arrays of structs containing atomics.)
var<storage, read_write> alive_counts: array<atomic<u32>>;
var<storage, read_write> birth_counts: array<atomic<u32>>;
// 16 regions per grid, flattened: index = grid_idx * 16 + region.
var<storage, read_write> region_alive: array<atomic<u32>>;
// Rule tables: 9 f32 probabilities (indices 0-8, Moore neighborhood max).
// spawn_table[n] = probability of dead cell becoming alive with n live neighbors.
// keep_table[n] = probability of alive cell surviving with n live neighbors.
var<storage, read> spawn_table: array<f32>;
var<storage, read> keep_table: array<f32>;

fn get_bit(base: u32, idx: u32) -> u32 {
    let word = grids_in[base + idx / 32u];
    return (word >> (idx % 32u)) & 1u;
}

// --- Philox 2x32-10 counter-based RNG ---

const PHILOX_M: u32 = 0xD256D193u;
const PHILOX_W: u32 = 0x9E3779B9u;

fn mulhi(a: u32, b: u32) -> u32 {
    let a_lo = a & 0xFFFFu;
    let a_hi = a >> 16u;
    let b_lo = b & 0xFFFFu;
    let b_hi = b >> 16u;
    let lo_lo = a_lo * b_lo;
    let hi_lo = a_hi * b_lo;
    let lo_hi = a_lo * b_hi;
    let hi_hi = a_hi * b_hi;
    let mid = (lo_lo >> 16u) + (hi_lo & 0xFFFFu) + (lo_hi & 0xFFFFu);
    return hi_hi + (hi_lo >> 16u) + (lo_hi >> 16u) + (mid >> 16u);
}

fn philox2x32(counter: vec2<u32>, key: u32) -> vec2<u32> {
    var c = counter;
    var k = key;
    for (var i = 0u; i < 10u; i++) {
        let hi = mulhi(PHILOX_M, c.x);
        let lo = PHILOX_M * c.x;
        c = vec2<u32>(hi ^ c.y ^ k, lo);
        k += PHILOX_W;
    }
    return c;
}

fn rand_f32(grid_idx: u32, step: u32, cell_idx: u32) -> f32 {
    let result = philox2x32(vec2<u32>(cell_idx, step), grid_idx);
    return f32(result.x >> 8u) / 16777216.0;
}

@compute @workgroup_size(256)
fn ca_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell_idx = gid.x;
    let grid_idx = gid.y;

    if grid_idx >= params.num_grids {
        return;
    }

    let x = cell_idx % params.grid_width;
    let y = cell_idx / params.grid_width;
    if y >= params.grid_height {
        return;
    }

    let base = grid_idx * params.words_per_grid;

    // Count Moore neighborhood (8 neighbors), honoring the boundary mode
    let width = i32(params.grid_width);
    let height = i32(params.grid_height);
    var count: u32 = 0u;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            if dx == 0 && dy == 0 {
                continue;
            }
            var nx = i32(x) + dx;
            var ny = i32(y) + dy;
            if params.boundary_mode == 1u {
                if nx < 0 || nx >= width || ny < 0 || ny >= height {
                    continue;
                }
            } else {
                nx = (nx + width) % width;
                ny = (ny + height) % height;
            }
            let ni = u32(ny) * params.grid_width + u32(nx);
            count += get_bit(base, ni);
        }
    }

    // Current cell state
    let ci = y * params.grid_width + x;
    let alive = get_bit(base, ci);

    var new_alive: u32 = 0u;
    if params.rule_mode == 0u {
        // Fast path: hardcoded B3/S23
        if alive == 1u {
            if count == 2u || count == 3u {
                new_alive = 1u;
            }
        } else {
            if count == 3u {
                new_alive = 1u;
            }
        }
    } else {
        // Table-driven probabilistic path
        let coin = rand_f32(grid_idx, params.current_step, ci);
        if alive == 1u {
            if coin < keep_table[count] {
                new_alive = 1u;
            }
        } else {
            if coin < spawn_table[count] {
                new_alive = 1u;
            }
        }
    }

    // Write output bit (atomic OR since multiple threads share a word)
    if new_alive == 1u {
        atomicOr(&grids_out[base + ci / 32u], 1u << (ci % 32u));
        atomicAdd(&alive_counts[grid_idx], 1u);

        if alive == 0u {
            atomicAdd(&birth_counts[grid_idx], 1u);
        }

        // Accumulate into 4x4 spatial region
        let rx = x * 4u / params.grid_width;
        let ry = y * 4u / params.grid_height;
        atomicAdd(&region_alive[grid_idx * 16u + ry * 4u + rx], 1u);
    }
}
