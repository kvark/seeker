use crate::sim::{
    Conclusion, Data, PopulationKind, Probability, ProbabilityTable, Simulation, Snap, Weight,
};
use rand::{rngs::ThreadRng, Rng as _};
use std::{mem, ops::Range};

const UPDATE_FREQUENCY: usize = 64;
const CHANNEL_BOUND: usize = 200;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Configuration {
    max_iterations: usize,
    max_active: usize,
    max_in_flight: usize,
    size_power: Range<usize>,
    probability_step: Probability,
    max_probability_weight: Weight,
}

pub struct Experiment {
    pub id: usize,
    snap: Snap,
    pub conclusion: Option<Conclusion>,
    pub steps: usize,
    pub fit: usize,
}

struct TaskStatus {
    experiment_id: usize,
    step: usize,
    conclusion: Option<Conclusion>,
}

pub struct Laboratory {
    config: Configuration,
    rng: ThreadRng,
    sender_origin: crossbeam_channel::Sender<TaskStatus>,
    receiver: crossbeam_channel::Receiver<TaskStatus>,
    choir: choir::Choir,
    // Better destroy them after the channel, so that workers
    // can see that this end is gone.
    _workers: Vec<choir::WorkerHandle>,
    experiments: Vec<Experiment>,
    next_id: usize,
}

pub enum LabResult {
    Normal,
    Found(Snap),
    End,
}

impl Laboratory {
    pub fn new(config: Configuration) -> Self {
        let mut choir = choir::Choir::new();
        let w1 = choir.add_worker("w1");
        let w2 = choir.add_worker("w2");
        let (sender_origin, receiver) = crossbeam_channel::bounded(CHANNEL_BOUND);
        Self {
            config,
            rng: ThreadRng::default(),
            sender_origin,
            receiver,
            choir,
            _workers: vec![w1, w2],
            experiments: Vec::new(),
            next_id: 0,
        }
    }

    pub fn experiments(&self) -> &[Experiment] {
        &self.experiments
    }

    pub fn progress_percent(&self) -> usize {
        self.next_id * 100 / self.config.max_iterations
    }

    pub fn best_candidate(&self) -> &Snap {
        &self.experiments[0].snap
    }

    pub fn add_experiment(&mut self, snap: Snap) {
        let id = self.next_id;
        self.next_id += 1;
        let mut sim = Simulation::new(&snap);
        let sender = self.sender_origin.clone();

        self.experiments.push(Experiment {
            id,
            snap,
            conclusion: None,
            steps: 0,
            fit: 0,
        });

        self.choir.add_task(move || loop {
            match sim.advance() {
                Ok(_) if sim.progress() % UPDATE_FREQUENCY == 0 => {
                    if sender
                        .send(TaskStatus {
                            experiment_id: id,
                            step: sim.progress(),
                            conclusion: None,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(_) => {}
                Err(conclusion) => {
                    let _ = sender.send(TaskStatus {
                        experiment_id: id,
                        step: sim.progress(),
                        conclusion: Some(conclusion),
                    });
                    return;
                }
            }
        });
    }

    pub fn update(&mut self) -> LabResult {
        while let Ok(progress) = self.receiver.try_recv() {
            let mut experiment = self
                .experiments
                .iter_mut()
                .find(|exp| exp.id == progress.experiment_id)
                .unwrap();
            assert!(experiment.conclusion.is_none());
            experiment.steps = progress.step;
            if let Some(conclusion) = progress.conclusion {
                let power = mem::size_of::<usize>() * 8 - progress.step.leading_zeros() as usize;
                experiment.conclusion = Some(conclusion);
                experiment.fit = match conclusion {
                    Conclusion::Extinct => power,
                    Conclusion::Indeterminate => 2 * power,
                    Conclusion::Stable(PopulationKind::Intra) => {
                        return LabResult::Found(experiment.snap.clone());
                    }
                    Conclusion::Stable(PopulationKind::Extra) => power,
                };
            }
        }

        self.experiments
            .sort_by_key(|experiment| -(experiment.fit as isize));
        let retain_cutoff = self
            .experiments
            .get(self.config.max_active)
            .map_or(0, |ex| ex.fit);
        let best_id = self.experiments[0].id;
        self.experiments
            .retain(|ex| ex.conclusion.is_none() || ex.fit > retain_cutoff || ex.id == best_id);

        if self.next_id >= self.config.max_iterations {
            if self.experiments.len() == self.config.max_active {
                return LabResult::End;
            }
        } else if self.experiments.len() < self.config.max_in_flight {
            let fit_sum = self.experiments.iter().map(|ex| ex.fit).sum::<usize>();

            let mut snap = if fit_sum > 0 {
                let mut cutoff = self.rng.gen_range(0..fit_sum);
                self.experiments
                    .iter()
                    .find_map(move |ex| {
                        if ex.fit > cutoff {
                            Some(ex.snap.clone())
                        } else {
                            cutoff -= ex.fit;
                            None
                        }
                    })
                    .unwrap()
            } else {
                self.experiments[0].snap.clone()
            };

            self.mutate_snap(&mut snap);
            self.add_experiment(snap);
        }

        LabResult::Normal
    }

    fn mutate_probabilities(&mut self, probabilities: &mut ProbabilityTable) {
        let index = self.rng.gen_range(0..=self.config.max_probability_weight);
        let value = probabilities.entry(index).or_insert(0.0);
        let left = *value - self.config.probability_step;
        let right = *value + self.config.probability_step;
        *value = if left < 0.0 {
            right
        } else if right < 1.0 {
            right
        } else {
            [left, right][self.rng.gen_range(0..2)]
        };
    }

    fn mutate_snap(&mut self, snap: &mut Snap) {
        match self.rng.gen_range(0..4) {
            0 => {
                self.mutate_probabilities(&mut snap.rules.spawn);
            }
            1 => {
                self.mutate_probabilities(&mut snap.rules.keep);
            }
            2 => {
                let row_index = self.rng.gen_range(0..snap.rules.kernel.len());
                let row = &mut snap.rules.kernel[row_index];
                let candidates = row
                    .char_indices()
                    .filter(|(_, c)| c.is_numeric())
                    .collect::<Vec<_>>();
                let (byte_offset, ch) = candidates[self.rng.gen_range(0..candidates.len())];
                let other = if ch == '0' {
                    '1' as u8
                } else if ch == '5' {
                    '4' as u8
                } else {
                    [ch as u8 - 1, ch as u8 + 1][self.rng.gen_range(0..2)]
                };
                unsafe {
                    row.as_bytes_mut()[byte_offset] = other;
                }
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
    }
}
