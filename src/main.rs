use rand::Rng as _;
use std::{io::Write, num::NonZeroU32};
use terminal::Action;

mod grid;

use grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

struct Simulation {
    grids: [Grid; 2],
    current_index: usize,
}

impl Simulation {
    fn new(width: Coordinate, height: Coordinate) -> Self {
        Self {
            grids: [Grid::new(width, height), Grid::new(width, height)],
            current_index: 0,
        }
    }

    fn current(&self) -> &Grid {
        self.grids.get(self.current_index).unwrap()
    }

    fn current_mut(&mut self) -> &mut Grid {
        self.grids.get_mut(self.current_index).unwrap()
    }

    fn advance(&mut self) {
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
                                avg_velocity[0] += denom * blend(xa as f32, cell.avg_velocity[0]);
                                avg_velocity[1] += denom * blend(ya as f32, cell.avg_velocity[1]);
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
                        avg_velocity: cell.avg_velocity,
                    }),
                    Some(_) => None,
                }
            }
        }
    }
}

fn draw<W: Write>(grid: &Grid, term: &mut terminal::Terminal<W>) {
    let size = grid.size();
    let mut string = String::new();
    for y in 0..size.1 {
        string.clear();
        for x in 0..size.0 {
            string.push(if grid.get(x, y).is_some() { '█' } else { ' ' });
        }
        term.batch(Action::MoveCursorTo(0, y as u16)).unwrap();
        term.write(string.as_bytes()).unwrap();
    }
    term.flush_batch().unwrap();
}

fn main() {
    let mut terminal = terminal::stdout();
    let mut sim = Simulation::new(10, 10);
    if false {
        let grid = sim.current_mut();
        let size = grid.size();
        let mut rng = rand::thread_rng();
        for _ in 0..size.0 * size.1 / 2 {
            grid.init(rng.gen::<Coordinate>().abs(), rng.gen::<Coordinate>().abs());
        }
    } else {
        let grid = sim.current_mut();
        // glider
        grid.init(1, 3);
        grid.init(2, 3);
        grid.init(3, 3);
        grid.init(3, 2);
        grid.init(2, 1);
    }

    terminal.act(Action::EnterAlternateScreen).unwrap();
    terminal.act(Action::EnableMouseCapture).unwrap();
    terminal.act(Action::EnableRawMode).unwrap();
    terminal.act(Action::HideCursor).unwrap();

    //terminal.act(Action::SetTerminalSize(size.0as u16, size.1 as u16));
    terminal
        .act(Action::ClearTerminal(terminal::Clear::All))
        .unwrap();

    draw(sim.current(), &mut terminal);

    loop {
        match terminal.get(terminal::Value::Event(None)) {
            Err(_) => break,
            Ok(terminal::Retrieved::Event(None))
            | Ok(terminal::Retrieved::TerminalSize(..))
            | Ok(terminal::Retrieved::CursorPosition(..)) => continue,
            Ok(terminal::Retrieved::Event(Some(event))) => match event {
                terminal::Event::Key(key_event) => match key_event.code {
                    terminal::KeyCode::Esc => {
                        break;
                    }
                    _ => {
                        sim.advance();
                        draw(sim.current(), &mut terminal);
                    }
                },
                terminal::Event::Mouse(terminal::MouseEvent::Down(_, x, y, _)) => {
                    let grid = sim.current();
                    terminal
                        .batch(Action::MoveCursorTo(0, y as u16 + 2))
                        .unwrap();
                    let msg = format!("{:?}", grid.get(x as Coordinate, y as Coordinate));
                    terminal.write(msg.as_bytes()).unwrap();
                }
                _ => {}
            },
        }
    }

    terminal.act(Action::ResetColor).unwrap();
    terminal.act(Action::ShowCursor).unwrap();
    terminal.act(Action::DisableMouseCapture).unwrap();
    terminal.act(Action::DisableRawMode).unwrap();
    terminal.act(Action::LeaveAlternateScreen).unwrap();
}
