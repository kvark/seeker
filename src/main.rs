use crossterm::{self as ct, ExecutableCommand as _};
use rand::Rng as _;
use std::io::Write;

mod grid;
mod sim;

const FULL: &'static str = "█";

fn draw<W: Write>(grid: &grid::Grid, term: &mut W) {
    use crossterm::style::Color;

    let size = grid.size();
    for y in 0..size.1 {
        for x in 0..size.0 {
            term.execute(ct::cursor::MoveTo(x as u16, y as u16))
                .unwrap();
            let (symbol, color) = if let Some(cell) = grid.get(x, y) {
                let velocity = cell.avg_velocity[0] * cell.avg_velocity[0]
                    + cell.avg_velocity[1] * cell.avg_velocity[1];
                let color = if velocity <= 0.04 {
                    Color::Red
                } else if velocity <= 0.16 {
                    Color::Green
                } else {
                    Color::Blue
                };
                (FULL, color)
            } else {
                (" ", Color::Reset)
            };
            term.execute(ct::style::SetForegroundColor(color)).unwrap();
            term.write(symbol.as_bytes()).unwrap();
        }
    }
}

fn main() {
    let mut terminal = std::io::stdout();
    let mut sim = sim::Simulation::new(60, 30);
    if true {
        let grid = sim.current_mut();
        let size = grid.size();
        let mut rng = rand::thread_rng();
        for _ in 0..size.0 * size.1 / 2 {
            grid.init(rng.gen(), rng.gen());
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

    terminal
        .execute(ct::terminal::EnterAlternateScreen)
        .unwrap();
    terminal.execute(ct::event::EnableMouseCapture).unwrap();
    ct::terminal::enable_raw_mode().unwrap();
    terminal.execute(ct::cursor::Hide).unwrap();

    //terminal.act(Action::SetTerminalSize(size.0as u16, size.1 as u16));
    terminal
        .execute(ct::terminal::Clear(ct::terminal::ClearType::All))
        .unwrap();

    draw(sim.current(), &mut terminal);

    loop {
        use crossterm::event as ev;
        match ct::event::read() {
            Err(_) => break,
            Ok(ev::Event::Key(event)) => match event.code {
                ev::KeyCode::Esc => {
                    break;
                }
                ev::KeyCode::Char(' ') => {
                    sim.advance();
                    draw(sim.current(), &mut terminal);
                }
                _ => {}
            },
            Ok(ev::Event::Mouse(ev::MouseEvent {
                kind: ev::MouseEventKind::Down(_),
                column,
                row,
                modifiers: _,
            })) => {
                let grid = sim.current();
                let cell = grid.get(column as _, row as _);
                // mark the current cell as selected
                terminal
                    .execute(ct::style::SetForegroundColor(ct::style::Color::Yellow))
                    .unwrap();
                terminal
                    .execute(ct::cursor::MoveTo(column as u16, row as u16))
                    .unwrap();
                let symbol = if cell.is_some() { FULL } else { " " };
                terminal.write(symbol.as_bytes()).unwrap();
                // print out extra info
                terminal
                    .execute(ct::cursor::MoveTo(0, grid.size().1 as u16 + 2))
                    .unwrap();
                terminal
                    .execute(ct::style::SetForegroundColor(ct::style::Color::Black))
                    .unwrap();
                terminal
                    .execute(ct::terminal::Clear(ct::terminal::ClearType::CurrentLine))
                    .unwrap();
                write!(terminal, "{:?}", cell).unwrap();
                terminal.flush().unwrap();
            }
            Ok(ev::Event::Resize(..)) | Ok(ev::Event::Mouse(..)) => {}
        }
    }

    terminal.execute(ct::style::ResetColor).unwrap();
    terminal.execute(ct::cursor::Show).unwrap();
    ct::terminal::disable_raw_mode().unwrap();
    terminal.execute(ct::event::DisableMouseCapture).unwrap();
    terminal
        .execute(ct::terminal::LeaveAlternateScreen)
        .unwrap();
}
