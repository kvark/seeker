use crossterm::{cursor, event, style, terminal, ExecutableCommand as _};
use rand::Rng as _;
use std::num::NonZeroU32;

mod grid;

use grid::{Cell, Coordinate, Grid};

const BLEND_FACTOR: f32 = 0.2;
fn blend(new: f32, old: f32) -> f32 {
    new * BLEND_FACTOR + old * (1.0 - BLEND_FACTOR)
}

fn main() {
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

    std::io::stdout()
        .execute(terminal::EnterAlternateScreen)
        .unwrap()
        .execute(cursor::Hide)
        .unwrap();

    terminal::enable_raw_mode().unwrap();

    std::io::stdout()
        .execute(terminal::Clear(terminal::ClearType::All))
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
            std::io::stdout()
                .execute(cursor::MoveTo(0, y as u16))
                .unwrap()
                .execute(style::Print(&string))
                .unwrap();
        }

        if let event::Event::Key(event::KeyEvent { code, .. }) = event::read().unwrap() {
            match code {
                event::KeyCode::Esc => {
                    break;
                }
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

    terminal::disable_raw_mode().unwrap();

    std::io::stdout()
        .execute(style::ResetColor)
        .unwrap()
        .execute(cursor::Show)
        .unwrap()
        .execute(terminal::LeaveAlternateScreen)
        .unwrap();
}
