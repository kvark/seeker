use rand::RngCore as _;
use std::{collections::HashMap, num::NonZeroU32};

use crate::grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

type Weight = u32;
type ProbabilityTable = HashMap<Weight, f32>;

#[derive(Debug, Default, Eq, Hash, PartialEq)]
struct Coordinates {
    x: Coordinate,
    y: Coordinate,
}

#[derive(Debug, Default)]
struct Rules {
    kernel: HashMap<Coordinates, Weight>,
    spawn: ProbabilityTable,
    keep: ProbabilityTable,
}

impl Rules {
    fn new_conways_life() -> Self {
        let mut rules = Self::default();
        for x in [-1, 0, 1] {
            for y in [-1, 0, 1] {
                if x != 0 || y != 0 {
                    rules.kernel.insert(Coordinates { x, y }, 1);
                }
            }
        }
        rules.spawn.insert(3, 1.0);
        rules.keep.insert(2, 1.0);
        rules.keep.insert(3, 1.0);
        rules
    }
}

pub struct Simulation {
    grids: [Grid; 2],
    grid_index: usize,
    rules: Rules,
    rng: rand::rngs::ThreadRng,
}

impl Simulation {
    pub fn new(width: Coordinate, height: Coordinate) -> Self {
        Self {
            grids: [Grid::new(width, height), Grid::new(width, height)],
            grid_index: 0,
            rules: Rules::new_conways_life(),
            rng: rand::thread_rng(),
        }
    }

    pub fn current(&self) -> &Grid {
        self.grids.get(self.grid_index).unwrap()
    }

    pub fn current_mut(&mut self) -> &mut Grid {
        self.grids.get_mut(self.grid_index).unwrap()
    }

    pub fn advance(&mut self) {
        let prev_index = self.grid_index;
        self.grid_index = (self.grid_index + 1) % self.grids.len();

        let (grids_before, grids_middle) = self.grids.split_at_mut(self.grid_index);
        let (grids_current, grids_after) = grids_middle.split_at_mut(1);
        let cur = grids_current.first_mut().unwrap();
        let prev = if prev_index < self.grid_index {
            grids_before.first().unwrap()
        } else {
            grids_after.last().unwrap()
        };
        let size = cur.size();

        for y in 0..size.1 {
            for x in 0..size.0 {
                let score = self
                    .rules
                    .kernel
                    .iter()
                    .map(
                        |(offsets, &weight)| match prev.get(x + offsets.x, y + offsets.y) {
                            Some(_) => weight,
                            None => 0,
                        },
                    )
                    .sum::<Weight>();
                let coin = (self.rng.next_u32() >> 8) as f32 / (1 << 24) as f32;

                *cur.mutate(x, y) = match prev.get(x, y) {
                    None => match self.rules.spawn.get(&score) {
                        Some(&chance) if coin < chance => {
                            let mut avg_breed_age = 0.0;
                            let mut avg_velocity = [0.0; 2];
                            for (offsets, &weight) in self.rules.kernel.iter() {
                                if let Some(cell) = prev.get(x + offsets.x, y + offsets.y) {
                                    let denom = weight as f32 / score as f32;
                                    avg_breed_age +=
                                        denom * blend(cell.age.get() as f32, cell.avg_breed_age);
                                    avg_velocity[0] +=
                                        denom * blend(-offsets.x as f32, cell.avg_velocity[0]);
                                    avg_velocity[1] +=
                                        denom * blend(-offsets.y as f32, cell.avg_velocity[1]);
                                }
                            }
                            Some(Cell {
                                age: NonZeroU32::new(1).unwrap(),
                                avg_breed_age,
                                avg_velocity,
                            })
                        }
                        _ => None,
                    },
                    Some(cell) => match self.rules.keep.get(&score) {
                        Some(&chance) if coin < chance => Some(Cell {
                            age: NonZeroU32::new(cell.age.get() + 1).unwrap(),
                            avg_breed_age: cell.avg_breed_age,
                            avg_velocity: [
                                blend(0.0, cell.avg_velocity[0]),
                                blend(0.0, cell.avg_velocity[1]),
                            ],
                        }),
                        _ => None,
                    },
                };
            }
        }
    }
}
