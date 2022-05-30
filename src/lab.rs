use crate::sim::{
    Conclusion, Data, PopulationKind, Probability, ProbabilityTable, Simulation, Snap, Weight,
};
use rand::{rngs::ThreadRng, Rng as _};
use std::{mem, ops::Range};

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Configuration {
    max_active: usize,
    max_iterations: usize,
    size_power: Range<usize>,
    probability_step: Probability,
    max_probability_weight: Weight,
}

pub struct Experiment {
    pub index: usize,
    snap: Snap,
    conclusion: Conclusion,
    pub steps: usize,
    pub fit: usize,
}

pub struct Laboratory {
    config: Configuration,
    rng: ThreadRng,
    experiments: Vec<Experiment>,
    iteration: usize,
    next_index: usize,
}

pub enum LabResult {
    Normal,
    Found(Snap),
    End,
}

impl Laboratory {
    pub fn new(config: Configuration) -> Self {
        Self {
            config,
            rng: ThreadRng::default(),
            experiments: Vec::new(),
            iteration: 0,
            next_index: 0,
        }
    }

    pub fn experiments(&self) -> &[Experiment] {
        &self.experiments
    }

    pub fn iteration(&self) -> usize {
        self.iteration
    }

    pub fn update_from(&mut self, start_index: usize) {
        for experiment in self.experiments[start_index..].iter_mut() {
            assert_eq!(experiment.fit, 0);
            let mut sim = Simulation::new(&experiment.snap);
            let conclusion = loop {
                if let Err(conclusion) = sim.advance() {
                    break conclusion;
                }
            };

            experiment.conclusion = conclusion;
            experiment.steps = sim.progress();
            let power = mem::size_of::<usize>() * 8 - sim.progress().leading_zeros() as usize;
            experiment.fit = match conclusion {
                Conclusion::Extinct => power,
                Conclusion::Indeterminate => 2 * power,
                Conclusion::Stable(PopulationKind::Intra) => 100,
                Conclusion::Stable(PopulationKind::Extra) => power,
            };
        }
    }

    pub fn add_experiment(&mut self, snap: Snap) {
        self.experiments.push(Experiment {
            index: self.next_index,
            snap,
            conclusion: Conclusion::Indeterminate,
            steps: 0,
            fit: 0,
        });
        self.next_index += 1;
    }

    fn mutate_probabilities(&mut self, probabilities: &mut ProbabilityTable) {
        let index = self.rng.gen_range(0..=self.config.max_probability_weight);
        let value = probabilities.entry(index).or_insert(0.0);
        if *value <= 0.0 {
            *value += self.config.probability_step;
        } else if *value >= 1.0 {
            *value -= self.config.probability_step;
        } else {
            let sign = (self.rng.gen::<u32>() & 1) as f32 * 2.0 - 1.0;
            *value += sign * self.config.probability_step;
        }
    }

    pub fn iterate(&mut self) -> LabResult {
        self.iteration += 1;
        if self.iteration > self.config.max_iterations {
            return LabResult::End;
        }
        let num_experiments = self.experiments.len();
        for i in 0..num_experiments {
            let mut snap = self.experiments[i].snap.clone();
            match self.rng.gen_range(0..3) {
                0 => {
                    self.mutate_probabilities(&mut snap.rules.spawn);
                }
                1 => {
                    self.mutate_probabilities(&mut snap.rules.keep);
                }
                _ => {
                    // size change
                    let size_power = self.rng.gen_range(self.config.size_power.clone());
                    match snap.data {
                        Data::Random {
                            ref mut width,
                            ref mut height,
                            alive_ratio: _,
                        } => {
                            *width = 1 << size_power;
                            *height = 1 << size_power;
                        }
                        Data::Grid(_) => {
                            log::error!("Unable to change grid size");
                        }
                    }
                }
            }
            self.add_experiment(snap);
        }

        self.update_from(num_experiments);
        // sort descending by fit
        self.experiments
            .sort_by_key(|experiment| -(experiment.fit as isize));
        self.experiments.truncate(self.config.max_active);

        if let Conclusion::Stable(PopulationKind::Intra) = self.experiments[0].conclusion {
            LabResult::Found(self.experiments[0].snap.clone())
        } else {
            LabResult::Normal
        }
    }
}
