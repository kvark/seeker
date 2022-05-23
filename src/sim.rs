use rand::RngCore as _;
use std::{collections::HashMap, num::NonZeroU32};

use crate::grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

type Weight = u32;
type Probability = f32;

#[derive(Debug, Default, Eq, Hash, PartialEq)]
struct Coordinates {
    x: Coordinate,
    y: Coordinate,
}

#[derive(Debug)]
struct Rules {
    kernel: HashMap<Coordinates, Weight>,
    spawn: Vec<Probability>,
    keep: Vec<Probability>,
}

impl Rules {
    fn new_conways_life() -> Self {
        let mut rules = Self {
            kernel: HashMap::default(),
            spawn: vec![0.0; 10],
            keep: vec![0.0; 10],
        };
        for x in [-1, 0, 1] {
            for y in [-1, 0, 1] {
                if x != 0 || y != 0 {
                    rules.kernel.insert(Coordinates { x, y }, 1);
                }
            }
        }
        rules.spawn[3] = 1.0;
        rules.keep[2] = 1.0;
        rules.keep[3] = 1.0;
        rules
    }

    fn get_sum(&self) -> Weight {
        self.kernel.values().cloned().sum::<Weight>()
    }

    fn are_valid(&self) -> bool {
        if self.kernel.get(&Coordinates::default()).is_some() {
            return false;
        }
        let sum = self.get_sum() as usize;
        if self.spawn.len() <= sum || !self.spawn.iter().all(|&prob| 0.0 <= prob && prob <= 1.0) {
            return false;
        }
        if self.keep.len() <= sum || !self.keep.iter().all(|&prob| 0.0 <= prob && prob <= 1.0) {
            return false;
        }
        true
    }
}

type ProbabilityTable = HashMap<Weight, Probability>;

#[derive(serde::Deserialize)]
struct RulesConfig {
    kernel: Vec<String>,
    spawn: ProbabilityTable,
    keep: ProbabilityTable,
}

#[derive(Debug)]
enum RulesConfigError {
    UnknownSymbol(char),
    MissingKernelCenter,
}

impl RulesConfig {
    fn parse(&self) -> Result<Rules, RulesConfigError> {
        let mut rules = Rules {
            kernel: HashMap::default(),
            spawn: Vec::new(),
            keep: Vec::new(),
        };
        let center = self
            .kernel
            .iter()
            .enumerate()
            .find_map(|(row, line)| {
                line.chars()
                    .position(|ch| ch == 'X')
                    .map(|column| Coordinates {
                        x: column as Coordinate,
                        y: row as Coordinate,
                    })
            })
            .ok_or(RulesConfigError::MissingKernelCenter)?;

        for (row, line) in self.kernel.iter().enumerate() {
            for (column, ch) in line.chars().enumerate() {
                match ch {
                    ' ' | 'X' => {}
                    '0'..='9' => {
                        let offset = Coordinates {
                            x: column as Coordinate - center.x,
                            y: row as Coordinate - center.y,
                        };
                        rules.kernel.insert(offset, ch as Weight - '0' as Weight);
                    }
                    _ => return Err(RulesConfigError::UnknownSymbol(ch)),
                }
            }
        }

        let sum = rules.get_sum() as usize;
        rules.spawn.resize(sum + 1, 0.0);
        rules.keep.resize(sum + 1, 0.0);

        for (&weight, &prob) in self.spawn.iter() {
            rules.spawn[weight as usize] = prob;
        }
        for (&weight, &prob) in self.keep.iter() {
            rules.keep[weight as usize] = prob;
        }
        Ok(rules)
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
                    None if coin < self.rules.spawn[score as usize] => {
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
                    Some(cell) if coin < self.rules.keep[score as usize] => Some(Cell {
                        age: NonZeroU32::new(cell.age.get() + 1).unwrap(),
                        avg_breed_age: cell.avg_breed_age,
                        avg_velocity: [
                            blend(0.0, cell.avg_velocity[0]),
                            blend(0.0, cell.avg_velocity[1]),
                        ],
                    }),
                    _ => None,
                };
            }
        }
    }
}
