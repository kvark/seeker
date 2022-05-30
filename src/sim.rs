use rand::{Rng as _, RngCore};
use std::{collections::HashMap, num::NonZeroU32};

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
}

impl Rules {
    fn _new_conways_life() -> Self {
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

#[derive(serde::Deserialize)]
pub enum Data {
    Random {
        width: Coordinate,
        height: Coordinate,
        alive_ratio: f32,
    },
    Grid(Vec<String>),
}

#[derive(Debug)]
enum DataParseError {
    UnknownSymbol(char),
    WrongLineLength,
}

impl Data {
    fn parse(&self, rng: &mut impl RngCore) -> Result<Grid, DataParseError> {
        match *self {
            Self::Random {
                width,
                height,
                alive_ratio,
            } => {
                let mut grid = Grid::new(Coordinates {
                    x: width,
                    y: height,
                });
                let count = (width as f32 * height as f32 * alive_ratio) as u32;
                for _ in 0..count {
                    grid.init(rng.gen(), rng.gen());
                }
                Ok(grid)
            }
            Self::Grid(ref lines) => {
                let size = Coordinates {
                    x: lines.first().ok_or(DataParseError::WrongLineLength)?.len() as Coordinate
                        * 4,
                    y: lines.len() as Coordinate,
                };
                let mut grid = Grid::new(size);
                for (y, line) in lines.iter().enumerate() {
                    for (x, ch) in line.chars().enumerate() {
                        let number = match ch {
                            '0'..='9' => ch as usize - '0' as usize,
                            'a'..='f' => 10 + ch as usize - 'a' as usize,
                            'A'..='F' => 10 + ch as usize - 'A' as usize,
                            _ => return Err(DataParseError::UnknownSymbol(ch)),
                        };
                        for z in 0..4 {
                            if number & 1 << z != 0 {
                                grid.init(x as Coordinate * 4 + z, y as Coordinate);
                            }
                        }
                    }
                }
                Ok(grid)
            }
        }
    }
}

type ProbabilityTable = HashMap<Weight, Probability>;

#[derive(serde::Deserialize)]
pub struct HumanRules {
    kernel: Vec<String>,
    spawn: ProbabilityTable,
    keep: ProbabilityTable,
}

#[derive(Debug)]
enum RulesParseError {
    UnknownSymbol(char),
    MissingKernelCenter,
}

impl HumanRules {
    fn parse(&self) -> Result<Rules, RulesParseError> {
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
            .ok_or(RulesParseError::MissingKernelCenter)?;

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
                    _ => return Err(RulesParseError::UnknownSymbol(ch)),
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

#[derive(serde::Deserialize)]
pub struct Snap {
    data: Data,
    rules: HumanRules,
    random_seed: u64,
    limits: Limits,
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
    limits: Limits,
    rng: rand::rngs::StdRng,
    step: usize,
    population: Population,
}

impl Simulation {
    pub fn new(snap: Snap) -> Self {
        let mut rng = rand::SeedableRng::seed_from_u64(snap.random_seed);
        let rules = snap.rules.parse().unwrap();
        let grid = snap.data.parse(&mut rng).unwrap();
        let size = grid.size();
        assert!(snap.limits.are_valid());

        Self {
            grids: [grid, Grid::new(size)],
            grid_index: 0,
            rules,
            limits: snap.limits,
            rng,
            step: 0,
            population: Population {
                kind: PopulationKind::Extra,
                age: 0,
            },
        }
    }

    pub fn _full_cycle(snap: Snap) -> Conclusion {
        let mut this = Self::new(snap);
        loop {
            if let Err(conclusion) = this.advance() {
                return conclusion;
            }
        }
    }

    pub fn progress(&self) -> usize {
        self.step
    }

    pub fn population(&self) -> &Population {
        &self.population
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn current(&self) -> &Grid {
        self.grids.get(self.grid_index).unwrap()
    }

    pub fn _current_mut(&mut self) -> &mut Grid {
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
        let (kind, max_age) = if analysis.alive_ratio > self.limits.min_extra_population {
            (PopulationKind::Extra, self.limits.max_extra_population_age)
        } else if analysis.alive_ratio > 0.0 {
            (PopulationKind::Intra, self.limits.max_intra_population_age)
        } else {
            return Err(Conclusion::Extinct);
        };
        if self.population.kind != kind {
            self.population.age = 0;
            self.population.kind = kind;
        }
        self.population.age += 1;
        if self.population.age > self.limits.max_steps {
            Err(Conclusion::Indeterminate)
        } else if self.population.age > max_age {
            Err(Conclusion::Stable(kind))
        } else {
            Ok(analysis)
        }
    }
}
