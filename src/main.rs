use crossterm::{cursor, event, style, terminal, ExecutableCommand as _};
use rand::Rng as _;
use std::mem;

mod grid;

use grid::{Coordinate, Grid};

fn main() {
    let size = (10, 10);
    let mut grids = {
        let mut grid = Grid::new(size.0, size.1);
        let other = grid.clone();
        if false {
            let mut rng = rand::thread_rng();
            for _ in 0..size.0 * size.1 / 2 {
                grid.set(
                    rng.gen::<Coordinate>().abs() % size.0,
                    rng.gen::<Coordinate>().abs() % size.1,
                );
            }
        } else {
            // glider
            grid.set(1, 3);
            grid.set(2, 3);
            grid.set(3, 3);
            grid.set(3, 2);
            grid.set(2, 1);
        }
        [grid, other]
    };

    std::io::stdout()
        .execute(terminal::EnterAlternateScreen)
        .unwrap()
        .execute(cursor::Hide)
        .unwrap();

    terminal::enable_raw_mode().unwrap();

    std::io::stdout()
        .execute(terminal::Clear(terminal::ClearType::All))
        .unwrap();

    loop {
        let (old, new) = grids.split_at_mut(1);
        let g = old.first().unwrap();
        let mut string = String::new();
        for y in 0..size.1 {
            string.clear();
            for x in 0..size.0 {
                string.push(if g.get(x, y) { '█' } else { ' ' });
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
        let other = new.first_mut().unwrap();
        other.clear();
        for y in 0..size.1 {
            for x in 0..size.0 {
                let neighbors = NEIGHBORS
                    .iter()
                    .filter_map(|&(xa, ya)| {
                        if g.get_wrapped(x + xa, y + ya) {
                            Some(())
                        } else {
                            None
                        }
                    })
                    .count();
                if neighbors == 3 || (neighbors == 2 && g.get(x, y)) {
                    other.set(x, y);
                }
            }
        }
        mem::swap(old.first_mut().unwrap(), other);
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
