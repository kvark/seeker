use crossterm::{cursor, event, style, terminal, ExecutableCommand as _};

mod grid;

use grid::Grid;

fn main() {
    let mut grid = Grid::new(10, 10);
    grid.set(1, 2, true);
    grid.set(5, 6, true);
    //print!("{}", grid);

    std::io::stdout()
        .execute(terminal::EnterAlternateScreen)
        .unwrap()
        .execute(cursor::Hide)
        .unwrap();

    terminal::enable_raw_mode().unwrap();

    std::io::stdout()
        .execute(terminal::Clear(terminal::ClearType::All))
        .unwrap();

    let mut string = String::new();
    for y in 0..10 {
        string.clear();
        for x in 0..10 {
            string.push(if grid.get(x, y) { '◼' } else { '◻' });
        }
        std::io::stdout()
            .execute(cursor::MoveTo(0, y as u16))
            .unwrap()
            .execute(style::Print(&string))
            .unwrap();
    }

    let _ = event::read();

    terminal::disable_raw_mode().unwrap();

    std::io::stdout()
        .execute(style::ResetColor)
        .unwrap()
        .execute(cursor::Show)
        .unwrap()
        .execute(terminal::LeaveAlternateScreen)
        .unwrap();
}
