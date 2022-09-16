use rand::{Rng as _, RngCore};
use rustc_hash::FxHashMap;
use std::{fmt, num::NonZeroU32};

use crate::grid::{Cell, Coordinate, Coordinates, Grid, GridAnalysis};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

pub type Weight = u32;
pub type Probability = f32;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Limits {
    pub max_steps: usize,
    pub update_weight: f32,
}

impl Limits {
    fn are_valid(&self) -> bool {
        if self.update_weight < 0.0 || self.update_weight > 1.0 {
            return false;
        }
        true
    }
}

#[derive(Debug)]
struct Rules {
    kernel: FxHashMap<Coordinates, Weight>,
    spawn: Vec<Probability>,
    keep: Vec<Probability>,
}

impl Rules {
    fn _new_conways_life() -> Self {
        let mut rules = Self {
            kernel: FxHashMap::default(),
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

    fn weight_sum(&self) -> Weight {
        self.kernel.values().cloned().sum::<Weight>()
    }

    fn are_valid(&self) -> bool {
        if self.kernel.get(&Coordinates::default()).is_some() {
            return false;
        }
        let sum = self.weight_sum() as usize;
        if self.spawn.len() <= sum || !self.spawn.iter().all(|prob| (0.0..=1.0).contains(prob)) {
            return false;
        }
        if self.keep.len() <= sum || !self.keep.iter().all(|prob| (0.0..=1.0).contains(prob)) {
            return false;
        }
        true
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum Data {
    Random {
        width: Coordinate,
        height: Coordinate,
        alive_ratio: f32,
    },
    Grid(Vec<String>),
}

#[derive(Debug)]
pub enum DataParseError {
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

    fn unparse(grid: &Grid) -> Self {
        let size = grid.size();
        assert_eq!(size.x % 4, 0);
        let mut lines = Vec::with_capacity(size.y as usize);
        for y in 0..size.y {
            let mut line = String::new();
            for x in 0..size.x / 4 {
                let mut number = 0u32;
                for z in 0..4 {
                    if grid.get(x * 4 + z, y).is_some() {
                        number |= 1 << z;
                    }
                }
                let ch_code = if number >= 10 {
                    'a' as u32 + number - 10
                } else {
                    '0' as u32 + number
                };
                line.push(char::from_u32(ch_code).unwrap());
            }
            lines.push(line);
        }
        Data::Grid(lines)
    }
}

pub type ProbabilityTable = FxHashMap<Weight, Probability>;

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct HumanRules {
    pub kernel: Vec<String>,
    pub spawn: ProbabilityTable,
    pub keep: ProbabilityTable,
}

#[derive(Debug)]
pub enum RulesParseError {
    UnknownSymbol(char),
    MissingKernelCenter,
    WeightOutOfBounds(Weight),
}

impl HumanRules {
    fn parse(&self) -> Result<Rules, RulesParseError> {
        let mut rules = Rules {
            kernel: FxHashMap::default(),
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

        let sum = rules.weight_sum() as usize;
        rules.spawn.resize(sum + 1, 0.0);
        rules.keep.resize(sum + 1, 0.0);

        for (&weight, &prob) in self.spawn.iter() {
            *rules
                .spawn
                .get_mut(weight as usize)
                .ok_or(RulesParseError::WeightOutOfBounds(weight))? = prob;
        }
        for (&weight, &prob) in self.keep.iter() {
            *rules
                .keep
                .get_mut(weight as usize)
                .ok_or(RulesParseError::WeightOutOfBounds(weight))? = prob;
        }

        if !rules.are_valid() {
            log::error!("Rules {:?} are invalid", rules);
        }
        Ok(rules)
    }

    fn unparse(rules: &Rules) -> Self {
        let mut hr = HumanRules::default();
        {
            // unparse the kernel
            let (mut xmin, mut xmax, mut ymin, mut ymax) = (0, 0, 0, 0);
            for offset in rules.kernel.keys() {
                xmin = xmin.min(offset.x);
                xmax = xmax.max(offset.x);
                ymin = ymin.min(offset.y);
                ymax = ymax.max(offset.y);
            }
            let kernel_size = Coordinates {
                x: 1 + xmax - xmin,
                y: 1 + ymax - ymin,
            };
            let mut proto_kernel = vec![' '; (kernel_size.x * kernel_size.y) as usize];
            proto_kernel[(-ymin * kernel_size.x - xmin) as usize] = 'X';
            for (offset, &weight) in rules.kernel.iter() {
                let index = (offset.y - ymin) * kernel_size.x + offset.x - xmin;
                proto_kernel[index as usize] = char::from_u32('0' as u32 + weight as u32).unwrap();
            }
            for proto_line in proto_kernel.chunks(kernel_size.x as usize) {
                hr.kernel.push(proto_line.iter().cloned().collect());
            }
        }
        for (w, &prob) in rules.spawn.iter().enumerate() {
            if w != 0 {
                hr.spawn.insert(w as Weight, prob);
            }
        }
        for (w, &prob) in rules.keep.iter().enumerate() {
            if w != 0 {
                hr.keep.insert(w as Weight, prob);
            }
        }
        hr
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub data: Data,
    pub rules: HumanRules,
    random_seed: u64,
    limits: Limits,
}

#[derive(Copy, Clone, Debug)]
pub struct Statistics {
    pub alive_ratio_average: f32,
    pub alive_ratio_variance: f32,
}

pub enum Conclusion {
    Extinct,
    Saturate,
    Done(Statistics, Snap),
    Crash,
}

impl fmt::Display for Conclusion {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Extinct => "Extinct".fmt(formatter),
            Self::Saturate => "Saturate".fmt(formatter),
            Self::Done(ref stats, _) => write!(
                formatter,
                "Done(alive_avg={}, alive_var={})",
                stats.alive_ratio_average, stats.alive_ratio_variance
            ),
            Self::Crash => "Crash".fmt(formatter),
        }
    }
}

impl Statistics {
    fn update(
        &mut self,
        analysis: GridAnalysis,
        limits: &Limits,
    ) -> Result<GridAnalysis, Conclusion> {
        if analysis.alive_ratio == 0.0 {
            return Err(Conclusion::Extinct);
        }

        let offset = analysis.alive_ratio - self.alive_ratio_average;
        let offset_squared_rel = offset * offset / self.alive_ratio_average.max(0.01);
        self.alive_ratio_average = limits.update_weight * analysis.alive_ratio
            + (1.0 - limits.update_weight) * self.alive_ratio_average;
        self.alive_ratio_variance = limits.update_weight * offset_squared_rel
            + (1.0 - limits.update_weight) * self.alive_ratio_variance;

        if self.alive_ratio_average > 0.9 {
            Err(Conclusion::Saturate)
        } else {
            Ok(analysis)
        }
    }
}

pub struct Simulation {
    grids: [Grid; 2],
    grid_index: usize,
    rules: Rules,
    limits: Limits,
    rng: rand::rngs::StdRng,
    random_seed: u64,
    step: usize,
    stats: Statistics,
}

#[derive(Debug)]
pub enum SnapError {
    RulesParse(RulesParseError),
    DataParse(DataParseError),
}
impl From<RulesParseError> for SnapError {
    fn from(error: RulesParseError) -> Self {
        Self::RulesParse(error)
    }
}
impl From<DataParseError> for SnapError {
    fn from(error: DataParseError) -> Self {
        Self::DataParse(error)
    }
}

impl Simulation {
    pub fn new(snap: &Snap) -> Result<Self, SnapError> {
        let mut rng = rand::SeedableRng::seed_from_u64(snap.random_seed);
        let rules = snap.rules.parse()?;
        let grid = snap.data.parse(&mut rng)?;
        let size = grid.size();
        assert!(snap.limits.are_valid());

        Ok(Self {
            grids: [grid, Grid::new(size)],
            grid_index: 0,
            rules,
            limits: snap.limits.clone(),
            rng,
            random_seed: snap.random_seed,
            step: 0,
            stats: Statistics {
                alive_ratio_average: match snap.data {
                    Data::Grid(_) => 0.0,
                    Data::Random { alive_ratio, .. } => alive_ratio,
                },
                alive_ratio_variance: 0.0,
            },
        })
    }

    pub fn save_snap(&self) -> Snap {
        Snap {
            data: Data::unparse(self.grid()),
            rules: HumanRules::unparse(&self.rules),
            random_seed: self.random_seed,
            limits: self.limits.clone(),
        }
    }

    pub fn stats(&self) -> &Statistics {
        &self.stats
    }

    pub fn last_step(&self) -> usize {
        self.step
    }

    pub fn random_seed(&self) -> u64 {
        self.random_seed
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn grid(&self) -> &Grid {
        self.grids.get(self.grid_index).unwrap()
    }

    pub fn advance(&mut self) -> Result<GridAnalysis, Conclusion> {
        let prev_index = self.grid_index;
        self.grid_index = (self.grid_index + 1) % self.grids.len();

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

        self.step += 1;
        if self.step > self.limits.max_steps {
            let snap = self.save_snap();
            Err(Conclusion::Done(self.stats, snap))
        } else {
            let analysis = grid.analyze();
            self.stats.update(analysis, &self.limits)
        }
    }
}
