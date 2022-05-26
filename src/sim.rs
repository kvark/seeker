use rand::{Rng as _, RngCore as _};
use std::{collections::HashMap, fs::File, num::NonZeroU32, path::PathBuf};

use crate::grid::{Cell, Coordinate, Coordinates, Grid, GridAnalysis};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

type Weight = u32;
type Probability = f32;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Limits {
    pub min_extra_population: f32,
    pub max_steps: usize,
    pub max_intra_population_age: usize,
    pub max_extra_population_age: usize,
}

impl Limits {
    fn are_valid(&self) -> bool {
        if self.min_extra_population <= 0.0 || self.min_extra_population > 1.0 {
            return false;
        }
        if self.max_steps
            <= self
                .max_intra_population_age
                .max(self.max_extra_population_age)
        {
            return false;
        }
        true
    }
}

#[derive(Debug)]
struct Rules {
    kernel: HashMap<Coordinates, Weight>,
    spawn: Vec<Probability>,
    keep: Vec<Probability>,
    limits: Limits,
}

impl Rules {
    fn _new_conways_life(limits: Limits) -> Self {
        let mut rules = Self {
            kernel: HashMap::default(),
            spawn: vec![0.0; 10],
            keep: vec![0.0; 10],
            limits,
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
        self.limits.are_valid()
    }
}

type ProbabilityTable = HashMap<Weight, Probability>;

#[derive(serde::Deserialize)]
pub struct HumanRules {
    size: (Coordinate, Coordinate),
    random_seed: u64,
    kernel: Vec<String>,
    spawn: ProbabilityTable,
    keep: ProbabilityTable,
    limits: Limits,
}

#[derive(Debug)]
enum HumanRulesError {
    UnknownSymbol(char),
    MissingKernelCenter,
}

impl HumanRules {
    fn parse(&self) -> Result<Rules, HumanRulesError> {
        let mut rules = Rules {
            kernel: HashMap::default(),
            spawn: Vec::new(),
            keep: Vec::new(),
            limits: self.limits.clone(),
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
            .ok_or(HumanRulesError::MissingKernelCenter)?;

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
                    _ => return Err(HumanRulesError::UnknownSymbol(ch)),
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

        assert!(rules.are_valid());
        Ok(rules)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PopulationKind {
    Intra,
    Extra,
}

pub struct Population {
    pub kind: PopulationKind,
    pub age: usize,
}

pub enum Conclusion {
    Extinct,
    Indeterminate,
    Stable(PopulationKind),
}

pub struct Simulation {
    grids: [Grid; 2],
    grid_index: usize,
    rules: Rules,
    rng: rand::rngs::StdRng,
    random_seed: u64,
    step: usize,
    population: Population,
}

impl Simulation {
    pub fn new() -> Self {
        let mut rules_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        rules_path.push("data");
        rules_path.push("rules.ron");
        let config = ron::de::from_reader(File::open(rules_path).unwrap()).unwrap();
        Self::from_human(config)
    }

    pub fn from_human(config: HumanRules) -> Self {
        let size = Coordinates {
            x: config.size.0,
            y: config.size.1,
        };
        Self {
            grids: [Grid::new(size), Grid::new(size)],
            grid_index: 0,
            rules: config.parse().unwrap(),
            rng: rand::SeedableRng::seed_from_u64(config.random_seed),
            random_seed: config.random_seed,
            step: 0,
            population: Population {
                kind: PopulationKind::Extra,
                age: 0,
            },
        }
    }

    pub fn _full_cycle(config: HumanRules) -> Conclusion {
        let mut this = Self::from_human(config);
        this.start();
        loop {
            if let Err(conclusion) = this.advance() {
                return conclusion;
            }
        }
    }

    pub fn start(&mut self) {
        let grid = self.grids.get_mut(self.grid_index).unwrap();
        let size = grid.size();
        for _ in 0..size.x * size.y / 2 {
            grid.init(self.rng.gen(), self.rng.gen());
        }
        self.step = 0;
        self.population.age = 0;
    }

    pub fn random_seed(&self) -> u64 {
        self.random_seed
    }

    pub fn progress(&self) -> usize {
        self.step
    }

    pub fn population(&self) -> &Population {
        &self.population
    }

    pub fn limits(&self) -> &Limits {
        &self.rules.limits
    }

    pub fn current(&self) -> &Grid {
        self.grids.get(self.grid_index).unwrap()
    }

    pub fn current_mut(&mut self) -> &mut Grid {
        self.grids.get_mut(self.grid_index).unwrap()
    }

    pub fn advance(&mut self) -> Result<GridAnalysis, Conclusion> {
        let prev_index = self.grid_index;
        self.grid_index = (self.grid_index + 1) % self.grids.len();
        self.step += 1;

        let (grids_before, grids_middle) = self.grids.split_at_mut(self.grid_index);
        let (grids_current, grids_after) = grids_middle.split_at_mut(1);
        let grid = grids_current.first_mut().unwrap();
        let prev = if prev_index < self.grid_index {
            grids_before.first().unwrap()
        } else {
            grids_after.last().unwrap()
        };
        let size = grid.size();

        for y in 0..size.y {
            for x in 0..size.x {
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

                *grid.mutate(x, y) = match prev.get(x, y) {
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

        let analysis = grid.analyze();
        let (kind, max_age) = if analysis.alive_ratio > self.rules.limits.min_extra_population {
            (
                PopulationKind::Extra,
                self.rules.limits.max_extra_population_age,
            )
        } else if analysis.alive_ratio > 0.0 {
            (
                PopulationKind::Intra,
                self.rules.limits.max_intra_population_age,
            )
        } else {
            return Err(Conclusion::Extinct);
        };
        if self.population.kind != kind {
            self.population.age = 0;
            self.population.kind = kind;
        }
        self.population.age += 1;
        if self.population.age > self.rules.limits.max_steps {
            Err(Conclusion::Indeterminate)
        } else if self.population.age > max_age {
            Err(Conclusion::Stable(kind))
        } else {
            Ok(analysis)
        }
    }
}
