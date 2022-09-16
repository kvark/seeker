use crate::sim::{Conclusion, Data, Probability, ProbabilityTable, Simulation, Snap, Weight};
use rand::{rngs::ThreadRng, Rng as _};
use std::{fs, io::Write as _, mem, ops::Range, path, sync::Arc};

const UPDATE_FREQUENCY: usize = 64;
const CHANNEL_BOUND: usize = 200;

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Configuration {
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
    choir: Arc<choir::Choir>,
    // Better destroy them after the channel, so that workers
    // can see that this end is gone.
    _workers: Vec<choir::WorkerHandle>,
    experiments: Vec<Experiment>,
    next_id: usize,
    active_dir: path::PathBuf,
    log: fs::File,
}

impl Laboratory {
    pub fn new(config: Configuration, active_dir_ref: impl AsRef<path::Path>) -> Self {
        let active_dir = path::PathBuf::from(active_dir_ref.as_ref());
        fs::create_dir_all(active_dir_ref).unwrap();
        {
            let file = fs::File::create(active_dir.join("config.ron")).unwrap();
            ron::ser::to_writer_pretty(file, &config, ron::ser::PrettyConfig::default()).unwrap();
        }
        let mut log = fs::File::create(active_dir.join("find.log")).unwrap();
        writeln!(log, "Seeker {}", env!("CARGO_PKG_VERSION")).unwrap();

        let choir = choir::Choir::new();
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
            active_dir,
            log,
        }
    }

    pub fn iteration(&self) -> usize {
        self.next_id
    }

    pub fn experiments(&self) -> &[Experiment] {
        &self.experiments
    }

    pub fn add_experiment(&mut self, snap: Snap, parent_id: usize) {
        let id = self.next_id;
        self.next_id += 1;
        let sender = self.sender_origin.clone();

        {
            let name = format!("e{}.ron", id);
            let file = fs::File::create(self.active_dir.join(name)).unwrap();
            ron::ser::to_writer_pretty(file, &snap, ron::ser::PrettyConfig::default()).unwrap();
        }

        let mut sim = match Simulation::new(&snap) {
            Ok(sim) => {
                writeln!(self.log, "Mutate E[{}] -> E[{}]", parent_id, self.next_id).unwrap();
                sim
            }
            Err(e) => {
                writeln!(self.log, "Skip E[{}]: {:?}", self.next_id, e).unwrap();
                return;
            }
        };

        self.experiments.push(Experiment {
            id,
            snap,
            conclusion: None,
            steps: 0,
            fit: 0,
        });

        self.choir.spawn("advance").init(move |_| loop {
            let step = sim.last_step() + 1;
            match sim.advance() {
                Ok(_) if step % UPDATE_FREQUENCY == 0 => {
                    if sender
                        .send(TaskStatus {
                            experiment_id: id,
                            step,
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
                        step,
                        conclusion: Some(conclusion),
                    });
                    return;
                }
            }
        });
    }

    pub fn update(&mut self) {
        while let Ok(status) = self.receiver.try_recv() {
            let max_fit = self
                .experiments
                .iter()
                .map(|exp| exp.fit)
                .max()
                .unwrap_or_default();

            let mut experiment = self
                .experiments
                .iter_mut()
                .find(|exp| exp.id == status.experiment_id)
                .unwrap();
            assert!(experiment.conclusion.is_none());
            experiment.steps = status.step;

            if let Some(conclusion) = status.conclusion {
                writeln!(
                    self.log,
                    "Conclude E[{}] as {} at step {}",
                    status.experiment_id, conclusion, status.step
                )
                .unwrap();

                experiment.fit = match conclusion {
                    Conclusion::Extinct | Conclusion::Saturate => {
                        mem::size_of::<usize>() * 8 - status.step.leading_zeros() as usize
                    }
                    Conclusion::Done(state, ref snap) => {
                        let fit = 100 - (60.0 * state.alive_ratio_average) as usize;
                        if fit > max_fit {
                            let name = format!("e{}-{}.ron", experiment.id, status.step);
                            let file = fs::File::create(self.active_dir.join(name)).unwrap();
                            ron::ser::to_writer_pretty(
                                file,
                                snap,
                                ron::ser::PrettyConfig::default(),
                            )
                            .unwrap();
                        }
                        fit
                    }
                    Conclusion::Crash => 0,
                };
                experiment.conclusion = Some(conclusion);
            }
        }

        self.experiments
            .sort_by_key(|experiment| -(experiment.fit as isize));
        let retain_cutoff = self
            .experiments
            .get(self.config.max_active)
            .map_or(0, |ex| ex.fit);
        let best_id = self.experiments[0].id;
        //TODO: different policies of the experiments generation
        // e.g. randomly choose (or resample) using `fit` as the weight
        self.experiments
            .retain(|ex| ex.conclusion.is_none() || ex.fit > retain_cutoff || ex.id == best_id);

        if self.experiments.len() < self.config.max_in_flight {
            let fit_sum = self.experiments.iter().map(|ex| ex.fit).sum::<usize>();

            let parent = if fit_sum > 0 {
                let mut cutoff = self.rng.gen_range(0..fit_sum);
                self.experiments
                    .iter()
                    .find(move |ex| {
                        if ex.fit > cutoff {
                            true
                        } else {
                            cutoff -= ex.fit;
                            false
                        }
                    })
                    .unwrap()
            } else {
                self.experiments.first().unwrap()
            };

            let mut snap = parent.snap.clone();
            let parent_id = parent.id;
            self.mutate_snap(&mut snap);
            self.add_experiment(snap, parent_id);
        }
    }

    fn mutate_probabilities(&mut self, probabilities: &mut ProbabilityTable) {
        let index = self.rng.gen_range(0..=self.config.max_probability_weight);
        let value = probabilities.entry(index).or_insert(0.0);
        let left = *value - self.config.probability_step;
        let right = *value + self.config.probability_step;
        *value = if left < 0.0 {
            right
        } else if right > 1.0 {
            left
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
                    b'1'
                } else if ch == '5' {
                    b'4'
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
