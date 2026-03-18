// Cellular automaton step shader (Game of Life B3/S23).
//
// Each invocation processes one cell in one grid.
// global_invocation_id.x = cell index (linear), .y = grid index.
//
// NOTE: No @group/@binding decorators — blade assigns them automatically
// by matching these variable names to the Rust ShaderData struct fields.

struct SimParams {
    grid_width: u32,
    grid_height: u32,
    words_per_grid: u32,
    num_grids: u32,
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

fn get_bit(base: u32, idx: u32) -> u32 {
    let word = grids_in[base + idx / 32u];
    return (word >> (idx % 32u)) & 1u;
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

    // Count Moore neighborhood (8 neighbors, toroidal wrap)
    var count: u32 = 0u;
    for (var dy: i32 = -1; dy <= 1; dy++) {
        for (var dx: i32 = -1; dx <= 1; dx++) {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = u32((i32(x) + dx + i32(params.grid_width)) % i32(params.grid_width));
            let ny = u32((i32(y) + dy + i32(params.grid_height)) % i32(params.grid_height));
            let ni = ny * params.grid_width + nx;
            count += get_bit(base, ni);
        }
    }

    // Current cell state
    let ci = y * params.grid_width + x;
    let alive = get_bit(base, ci);

    // B3/S23 rule
    var new_alive: u32 = 0u;
    if alive == 1u {
        if count == 2u || count == 3u {
            new_alive = 1u;
        }
    } else {
        if count == 3u {
            new_alive = 1u;
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
