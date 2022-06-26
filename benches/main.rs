use seeker::grid::{Coordinate, Coordinates, Grid};

use std::mem;

mod binary_grid {
    use seeker::grid::{Coordinate, Coordinates};
    use std::mem;

    type TileValue = u64;
    type TileIndex = u32;
    const TILE_SHIFT: u32 = 3;
    const TILE_MASK: Coordinate = (1 << TILE_SHIFT) - 1;

    struct InternalAddress {
        tile: usize,
        mask: TileValue,
    }

    #[derive(Clone)]
    pub struct Grid {
        size: Coordinates,
        size_in_tiles: (TileIndex, TileIndex),
        data: Box<[TileValue]>,
    }

    impl Grid {
        pub fn new(size: Coordinates) -> Self {
            assert!(1 << (TILE_SHIFT * 2) == mem::size_of::<TileValue>() * 8);
            let size_in_tiles = (
                ((size.x - 1) as TileIndex >> TILE_SHIFT) + 1,
                ((size.y - 1) as TileIndex >> TILE_SHIFT) + 1,
            );
            let data = vec![0; (size_in_tiles.0 * size_in_tiles.1) as usize].into_boxed_slice();
            Self {
                size,
                size_in_tiles,
                data,
            }
        }

        fn internal_address(&self, x: Coordinate, y: Coordinate) -> InternalAddress {
            debug_assert!(x >= 0 && x < self.size.x);
            debug_assert!(y >= 0 && y < self.size.y);
            let tile = (y >> TILE_SHIFT) as usize * self.size_in_tiles.1 as usize
                + (x >> TILE_SHIFT) as usize;
            let bit_index = ((y & TILE_MASK) << TILE_SHIFT) + (x & TILE_MASK);
            InternalAddress {
                tile,
                mask: 1 << bit_index,
            }
        }

        pub fn _clear(&mut self) {
            for v in self.data.iter_mut() {
                *v = 0;
            }
        }

        pub fn get(&self, x: Coordinate, y: Coordinate) -> bool {
            let ia = self.internal_address(x, y);
            self.data[ia.tile] & ia.mask != 0
        }
        pub fn _get_wrapped(&self, x: Coordinate, y: Coordinate) -> bool {
            let ia = self.internal_address(
                if x < 0 {
                    x + self.size.x
                } else if x >= self.size.x {
                    x - self.size.x
                } else {
                    x
                },
                if y < 0 {
                    y + self.size.y
                } else if y >= self.size.y {
                    y - self.size.y
                } else {
                    y
                },
            );
            self.data[ia.tile] & ia.mask != 0
        }

        pub fn set(&mut self, x: Coordinate, y: Coordinate) {
            let ia = self.internal_address(x, y);
            self.data[ia.tile] |= ia.mask;
        }

        pub fn unset(&mut self, x: Coordinate, y: Coordinate) {
            let ia = self.internal_address(x, y);
            self.data[ia.tile] &= !ia.mask;
        }
    }
}

trait Griddy: Sized {
    fn new(size: Coordinates) -> Self;
    fn get(&self, x: Coordinate, y: Coordinate) -> bool;
    fn set(&mut self, x: Coordinate, y: Coordinate);
    fn unset(&mut self, x: Coordinate, y: Coordinate);
}
impl Griddy for Grid {
    fn new(size: Coordinates) -> Self {
        Grid::new(size)
    }
    fn get(&self, x: Coordinate, y: Coordinate) -> bool {
        self.get(x, y).is_some()
    }
    fn set(&mut self, x: Coordinate, y: Coordinate) {
        self.init(x, y);
    }
    fn unset(&mut self, x: Coordinate, y: Coordinate) {
        *self.mutate(x, y) = None;
    }
}
impl Griddy for binary_grid::Grid {
    fn new(size: Coordinates) -> Self {
        binary_grid::Grid::new(size)
    }
    fn get(&self, x: Coordinate, y: Coordinate) -> bool {
        (self as &binary_grid::Grid).get(x, y)
    }
    fn set(&mut self, x: Coordinate, y: Coordinate) {
        (self as &mut binary_grid::Grid).set(x, y);
    }
    fn unset(&mut self, x: Coordinate, y: Coordinate) {
        (self as &mut binary_grid::Grid).unset(x, y);
    }
}

const SIZE: Coordinate = 1 << 8;

struct Sim<G> {
    grid: G,
    neighbors: Box<[u8]>,
}

impl<G: Griddy> Sim<G> {
    fn new(rng: &mut impl rand::Rng) -> Self {
        let mut grid = G::new(Coordinates { x: SIZE, y: SIZE });
        for _ in 0..SIZE * SIZE / 5 {
            grid.set(rng.gen_range(0..SIZE), rng.gen_range(0..SIZE));
        }
        Self {
            grid,
            neighbors: vec![0; (SIZE * SIZE) as usize].into_boxed_slice(),
        }
    }

    fn add(&mut self, x: Coordinate, y: Coordinate) {
        if x > 0 && x < SIZE && y > 0 && y < SIZE {
            self.neighbors[(y * SIZE + x) as usize] += 1;
        }
    }

    fn step(&mut self) {
        for y in 0..SIZE {
            for x in 0..SIZE {
                if self.grid.get(x, y) {
                    self.add(x - 1, y - 1);
                    self.add(x - 1, y);
                    self.add(x - 1, y + 1);
                    self.add(x, y - 1);
                    self.add(x, y + 1);
                    self.add(x + 1, y - 1);
                    self.add(x + 1, y);
                    self.add(x + 1, y + 1);
                }
            }
        }
        for y in 0..SIZE {
            for x in 0..SIZE {
                let neighbors = mem::replace(&mut self.neighbors[(y * SIZE + x) as usize], 0);
                if neighbors == 3 {
                    self.grid.set(x, y);
                } else if neighbors != 2 {
                    self.grid.unset(x, y);
                }
            }
        }
    }
}

pub fn grid_step(c: &mut criterion::Criterion) {
    let mut rng = rand::thread_rng();
    c.bench_function("fat", |b| {
        let mut sim = Sim::<Grid>::new(&mut rng);
        b.iter(|| sim.step())
    });
    c.bench_function("binary", |b| {
        let mut sim = Sim::<binary_grid::Grid>::new(&mut rng);
        b.iter(|| sim.step())
    });
}

criterion::criterion_group!(benches, grid_step);
criterion::criterion_main!(benches);
