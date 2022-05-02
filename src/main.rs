use rand::Rng as _;
use std::io::Write;
use terminal::{Action, Color};

mod grid;
mod sim;

fn draw<W: Write>(grid: &grid::Grid, term: &mut terminal::Terminal<W>) {
    let size = grid.size();
    for y in 0..size.1 {
        for x in 0..size.0 {
            term.batch(Action::MoveCursorTo(x as u16, y as u16))
                .unwrap();
            let (symbol, color) = if let Some(cell) = grid.get(x, y) {
                let color = if cell.avg_breed_age < 1.5 {
                    Color::Red
                } else if cell.avg_breed_age < 3.0 {
                    Color::Green
                } else {
                    Color::Blue
                };
                ("█", color)
            } else {
                (" ", Color::Reset)
            };
            term.batch(Action::SetForegroundColor(color)).unwrap();
            term.write(symbol.as_bytes()).unwrap();
        }
    }
    term.flush_batch().unwrap();
}

fn main() {
    let mut terminal = terminal::stdout();
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
                    let msg = format!("{:?}", grid.get(x as _, y as _));
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
