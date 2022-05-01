use rand::Rng as _;
use std::{io::Write as _, num::NonZeroU32};
use terminal::Action;

mod grid;

use grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

fn main() {
    let mut terminal = terminal::stdout();
    let size = (10, 10);
    let mut grids = [Grid::new(size.0, size.1), Grid::new(size.0, size.1)];

    if false {
        let mut rng = rand::thread_rng();
        for _ in 0..size.0 * size.1 / 2 {
            grids[0].init(rng.gen::<Coordinate>().abs(), rng.gen::<Coordinate>().abs());
        }
    } else {
        let g = grids.first_mut().unwrap();
        // glider
        g.init(1, 3);
        g.init(2, 3);
        g.init(3, 3);
        g.init(3, 2);
        g.init(2, 1);
    }

    terminal.act(Action::EnterAlternateScreen).unwrap();
    terminal.act(Action::EnableMouseCapture).unwrap();
    terminal.act(Action::EnableRawMode).unwrap();
    terminal.act(Action::HideCursor).unwrap();

    //terminal.act(Action::SetTerminalSize(size.0as u16, size.1 as u16));
    terminal
        .act(Action::ClearTerminal(terminal::Clear::All))
        .unwrap();

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

    let mut current_history_index = 0;
    loop {
        let mut string = String::new();
        for y in 0..size.1 {
            let g = &grids[current_history_index];
            string.clear();
            for x in 0..size.0 {
                string.push(if g.get(x, y).is_some() { '█' } else { ' ' });
            }
            terminal.batch(Action::MoveCursorTo(0, y as u16)).unwrap();
            terminal.write(string.as_bytes()).unwrap();
        }
        terminal.flush_batch().unwrap();

        if let Ok(terminal::Retrieved::Event(Some(event))) =
            terminal.get(terminal::Value::Event(None))
        {
            match event {
                terminal::Event::Key(key_event) => match key_event.code {
                    terminal::KeyCode::Esc => {
                        break;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let prev_histiroy_index = current_history_index;
        current_history_index = (current_history_index + 1) % grids.len();
        let (grids_before, grids_middle) = grids.split_at_mut(current_history_index);
        let (grids_current, grids_after) = grids_middle.split_at_mut(1);
        let cur = grids_current.first_mut().unwrap();
        let prev = if prev_histiroy_index < current_history_index {
            grids_before.first().unwrap()
        } else {
            grids_after.last().unwrap()
        };

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

    terminal.act(Action::ResetColor).unwrap();
    terminal.act(Action::ShowCursor).unwrap();
    terminal.act(Action::DisableMouseCapture).unwrap();
    terminal.act(Action::DisableRawMode).unwrap();
    terminal.act(Action::LeaveAlternateScreen).unwrap();
}
