use rand::Rng as _;
use std::io::Write;
use terminal::Action;

mod grid;
mod sim;

fn draw<W: Write>(grid: &grid::Grid, term: &mut terminal::Terminal<W>) {
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
    let mut sim = sim::Simulation::new(10, 10);
    if false {
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
