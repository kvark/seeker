mod grid;
mod sim;

struct GridRef<'a>(&'a grid::Grid);
impl tui::widgets::Widget for GridRef<'_> {
    fn render(self, area: tui::layout::Rect, buf: &mut tui::buffer::Buffer) {
        use tui::style::Color;

        for y in 0..area.height {
            for x in 0..area.width {
                let (symbol, color) = if let Some(cell) = self.0.get(x as _, y as _) {
                    let velocity = cell.avg_velocity[0] * cell.avg_velocity[0]
                        + cell.avg_velocity[1] * cell.avg_velocity[1];
                    let color = if velocity <= 0.04 {
                        Color::Red
                    } else if velocity <= 0.16 {
                        Color::Green
                    } else {
                        Color::Blue
                    };
                    ("█", color)
                } else {
                    (" ", Color::Reset)
                };

                let cell_index = (y + area.y) * buf.area.width + x + area.x;
                buf.content[cell_index as usize] = tui::buffer::Cell {
                    symbol: symbol.to_string(), // what a waste!
                    fg: color,
                    ..Default::default()
                };
            }
        }
    }
}

impl sim::Simulation {
    fn draw<B: tui::backend::Backend>(&self, frame: &mut tui::Frame<B>) {
        let rects = tui::layout::Layout::default()
            .constraints([tui::layout::Constraint::Percentage(100)].as_ref())
            .margin(1)
            .split(frame.size());

        let block = tui::widgets::Block::default()
            .borders(tui::widgets::Borders::ALL)
            .title(format!("Grid step-{}", self.progress()));
        let inner = block.inner(rects[0]);
        frame.render_widget(block, rects[0]);
        frame.render_widget(GridRef(self.current()), inner);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::ExecutableCommand as _;

    let mut stdout = std::io::stdout();
    let mut sim = sim::Simulation::new();
    if true {
        sim.start();
    } else {
        let grid = sim.current_mut();
        // glider
        grid.init(1, 3);
        grid.init(2, 3);
        grid.init(3, 3);
        grid.init(3, 2);
        grid.init(2, 1);
    }

    stdout.execute(crossterm::terminal::EnterAlternateScreen)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;
    crossterm::terminal::enable_raw_mode()?;
    let backend = tui::backend::CrosstermBackend::new(stdout);
    let mut terminal = tui::Terminal::new(backend)?;

    terminal.hide_cursor()?;
    terminal.draw(|f| sim.draw(f))?;

    loop {
        use crossterm::event as ev;
        match crossterm::event::read() {
            Err(_) => break,
            Ok(ev::Event::Resize(..)) => {
                terminal.draw(|f| sim.draw(f))?;
            }
            Ok(ev::Event::Key(event)) => match event.code {
                ev::KeyCode::Esc => {
                    break;
                }
                ev::KeyCode::Char(' ') => {
                    let meta = sim.advance();
                    if meta.num_alive == 0 {
                        sim.start();
                    }
                    terminal.draw(|f| sim.draw(f))?;
                }
                _ => {}
            },
            Ok(ev::Event::Mouse(ev::MouseEvent {
                kind: ev::MouseEventKind::Down(_),
                column: _,
                row: _,
                modifiers: _,
            })) => {
                /*
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
                    .execute(ct::cursor::MoveTo(0, grid.size().y as u16 + 2))
                    .unwrap();
                terminal
                    .execute(ct::style::SetForegroundColor(ct::style::Color::Black))
                    .unwrap();
                terminal
                    .execute(ct::terminal::Clear(ct::terminal::ClearType::CurrentLine))
                    .unwrap();
                write!(terminal, "{:?}", cell).unwrap();
                terminal.flush().unwrap();
                */
            }
            Ok(ev::Event::Mouse(..)) => {}
        }
    }

    crossterm::terminal::disable_raw_mode()?;
    terminal
        .backend_mut()
        .execute(crossterm::event::DisableMouseCapture)?;
    terminal
        .backend_mut()
        .execute(crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
