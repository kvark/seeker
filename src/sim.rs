use std::num::NonZeroU32;

use crate::grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

const NEIGHBORS: &[(Coordinate, Coordinate)] = &[
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

pub struct Simulation {
    grids: [Grid; 2],
    current_index: usize,
}

impl Simulation {
    pub fn new(width: Coordinate, height: Coordinate) -> Self {
        Self {
            grids: [Grid::new(width, height), Grid::new(width, height)],
            current_index: 0,
        }
    }

    pub fn current(&self) -> &Grid {
        self.grids.get(self.current_index).unwrap()
    }

    pub fn current_mut(&mut self) -> &mut Grid {
        self.grids.get_mut(self.current_index).unwrap()
    }

    pub fn advance(&mut self) {
        let prev_index = self.current_index;
        self.current_index = (self.current_index + 1) % self.grids.len();

        let (grids_before, grids_middle) = self.grids.split_at_mut(self.current_index);
        let (grids_current, grids_after) = grids_middle.split_at_mut(1);
        let cur = grids_current.first_mut().unwrap();
        let prev = if prev_index < self.current_index {
            grids_before.first().unwrap()
        } else {
            grids_after.last().unwrap()
        };
        let size = cur.size();

        for y in 0..size.1 {
            for x in 0..size.0 {
                let neighbors_count = NEIGHBORS
                    .iter()
                    .filter_map(|&(xa, ya)| prev.get(x + xa, y + ya))
                    .count();
                *cur.mutate(x, y) = match prev.get(x, y) {
                    None if neighbors_count == 3 => {
                        let mut avg_breed_age = 0.0;
                        let mut avg_velocity = [0.0; 2];
                        let denom = 1.0 / neighbors_count as f32;
                        for &(xa, ya) in NEIGHBORS.iter() {
                            if let Some(cell) = prev.get(x + xa, y + ya) {
                                avg_breed_age +=
                                    denom * blend(cell.age.get() as f32, cell.avg_breed_age);
                                avg_velocity[0] += denom * blend(-xa as f32, cell.avg_velocity[0]);
                                avg_velocity[1] += denom * blend(-ya as f32, cell.avg_velocity[1]);
                            }
                        }
                        Some(Cell {
                            age: NonZeroU32::new(1).unwrap(),
                            avg_breed_age,
                            avg_velocity,
                        })
                    }
                    None => None,
                    Some(cell) if neighbors_count >= 2 && neighbors_count <= 3 => Some(Cell {
                        age: NonZeroU32::new(cell.age.get() + 1).unwrap(),
                        avg_breed_age: cell.avg_breed_age,
                        avg_velocity: [
                            blend(0.0, cell.avg_velocity[0]),
                            blend(0.0, cell.avg_velocity[1]),
                        ],
                    }),
                    Some(_) => None,
                }
            }
        }
    }
}
