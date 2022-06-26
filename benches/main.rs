use seeker::grid::{Coordinate, Coordinates, Grid};

use std::mem;

const SIZE: Coordinate = 1 << 8;

struct Sim {
    grid: Grid,
    neighbors: Box<[u8]>,
}

impl Sim {
    fn new(rng: &mut impl rand::Rng) -> Self {
        let mut grid = Grid::new(Coordinates { x: SIZE, y: SIZE });
        for _ in 0..SIZE * SIZE / 5 {
            grid.init(rng.gen(), rng.gen());
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
                if self.grid.get(x, y).is_some() {
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
                    self.grid.init(x, y);
                } else if neighbors != 2 {
                    *self.grid.mutate(x, y) = None;
                }
            }
        }
    }
}

pub fn grid_step(c: &mut criterion::Criterion) {
    let mut rng = rand::thread_rng();
    c.bench_function("fat", |b| {
        let mut sim = Sim::new(&mut rng);
        b.iter(|| sim.step())
    });
}

criterion::criterion_group!(benches, grid_step);
criterion::criterion_main!(benches);
