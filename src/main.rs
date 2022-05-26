mod grid;
mod sim;

#[derive(Default)]
struct WidgetState {
    selection: Option<grid::Coordinates>,
    population_age: usize,
}

const POPULATION_CHANGE_THRESHOLD: f32 = 0.1;
const POPULATION_DANGER_THRESHOLD: f32 = 0.02;
const MAX_POPULATION_AGE_LOG: usize = 16;

struct GridWidget<'a> {
    grid: &'a grid::Grid,
    state: &'a WidgetState,
}
impl tui::widgets::Widget for GridWidget<'_> {
    fn render(self, area: tui::layout::Rect, buf: &mut tui::buffer::Buffer) {
        use tui::style::Color;

        for y in 0..area.height {
            for x in 0..area.width {
                let (symbol, color) = if let Some(cell) = self.grid.get(x as _, y as _) {
                    let velocity = cell.avg_velocity[0] * cell.avg_velocity[0]
                        + cell.avg_velocity[1] * cell.avg_velocity[1];
                    let color = if velocity <= 0.03 {
                        Color::Red
                    } else if velocity <= 0.10 {
                        Color::Green
                    } else {
                        Color::Blue
                    };
                    ("█", color)
                } else {
                    (" ", Color::Reset)
                };
                let background = if self.state.selection
                    == Some(grid::Coordinates {
                        x: (area.x + x) as _,
                        y: (area.y + y) as _,
                    }) {
                    Color::White
                } else {
                    Color::Reset
                };

                let cell_index = (y + area.y) * buf.area.width + x + area.x;
                buf.content[cell_index as usize] = tui::buffer::Cell {
                    symbol: symbol.to_string(), // what a waste!
                    fg: color,
                    bg: background,
                    ..Default::default()
                };
            }
        }
    }
}

impl sim::Simulation {
    fn draw<B: tui::backend::Backend>(&self, state: &WidgetState, frame: &mut tui::Frame<B>) {
        use tui::{
            layout as l,
            style::{Color, Style},
            text::{Span, Spans},
            widgets as w,
        };

        fn make_key_value(key: &str, value: String) -> Spans {
            Spans(vec![
                Span::styled(key, Style::default().fg(Color::DarkGray)),
                Span::raw(value),
            ])
        }

        let grid = self.current();
        let grid_size = grid.size();

        let top_rects = l::Layout::default()
            .direction(l::Direction::Horizontal)
            .constraints(
                [
                    l::Constraint::Min(grid_size.x as _),
                    l::Constraint::Percentage(15),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(frame.size());

        let grid_block = w::Block::default().borders(w::Borders::ALL).title("Grid");
        let inner = grid_block.inner(top_rects[0]);
        frame.render_widget(grid_block, top_rects[0]);
        frame.render_widget(GridWidget { grid, state }, inner);

        {
            let meta_rects = l::Layout::default()
                .direction(l::Direction::Vertical)
                .constraints(
                    [
                        l::Constraint::Min(3),
                        l::Constraint::Min(5),
                        l::Constraint::Min(4),
                    ]
                    .as_ref(),
                )
                .split(top_rects[1]);

            let para_size = w::Paragraph::new(vec![
                make_key_value("Size = ", format!("{}x{}", grid_size.x, grid_size.y)),
                make_key_value("Random = ", format!("{}", self.random_seed())),
            ])
            .block(w::Block::default().title("Info").borders(w::Borders::ALL))
            .wrap(w::Wrap { trim: false });
            frame.render_widget(para_size, meta_rects[0]);

            {
                let stat_block = w::Block::default().title("Stat").borders(w::Borders::ALL);
                let stat_rects = l::Layout::default()
                    .direction(l::Direction::Vertical)
                    .constraints(
                        [
                            l::Constraint::Min(1),
                            l::Constraint::Min(1),
                            l::Constraint::Min(1),
                        ]
                        .as_ref(),
                    )
                    .split(stat_block.inner(meta_rects[1]));
                frame.render_widget(stat_block, meta_rects[1]);

                let para_progress = w::Paragraph::new(vec![make_key_value(
                    "Progress = ",
                    format!("{}", self.progress()),
                )])
                .wrap(w::Wrap { trim: false });
                frame.render_widget(para_progress, stat_rects[0]);

                let analysis = grid.analyze();
                let occupancy_color = if analysis.alive_ratio > POPULATION_CHANGE_THRESHOLD {
                    Color::Blue
                } else if analysis.alive_ratio > POPULATION_DANGER_THRESHOLD {
                    Color::Green
                } else {
                    Color::Red
                };
                let occupancy = w::Gauge::default()
                    .gauge_style(Style::default().fg(occupancy_color))
                    // Square root brings more precision to lower occupancy,
                    // which is what we mostly care about.
                    .percent((100.0 * analysis.alive_ratio.sqrt()) as u16)
                    .label(format!(
                        "{}% alive",
                        (100.0 * analysis.alive_ratio) as usize
                    ));
                frame.render_widget(occupancy, stat_rects[1]);

                let age_log = std::mem::size_of::<usize>() * 8
                    - state.population_age.leading_zeros() as usize;
                let age_color = if age_log > 10 {
                    Color::White
                } else if age_log > 5 {
                    Color::Gray
                } else {
                    Color::DarkGray
                };
                let population_age = w::Gauge::default()
                    .gauge_style(Style::default().fg(age_color))
                    // Square root brings more precision to lower occupancy,
                    // which is what we mostly care about.
                    .percent((100 * age_log / MAX_POPULATION_AGE_LOG) as u16)
                    .label(format!("{} age", age_log));
                frame.render_widget(population_age, stat_rects[2]);
            }

            if let Some(coords) = state.selection {
                let x = coords.x - inner.x as grid::Coordinate;
                let y = coords.y - inner.y as grid::Coordinate;
                let mut text = vec![make_key_value("Coord = ", format!("{}x{}", x, y))];
                if let Some(cell) = grid.get(x, y) {
                    text.push(make_key_value("Age = ", format!("{}", cell.age.get())));
                }
                let para_selection = w::Paragraph::new(text)
                    .block(w::Block::default().title("Cell").borders(w::Borders::ALL))
                    .wrap(w::Wrap { trim: false });
                frame.render_widget(para_selection, meta_rects[2]);
            }
        }
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

    let mut state = WidgetState::default();
    terminal.hide_cursor()?;
    terminal.draw(|f| sim.draw(&state, f))?;

    loop {
        use crossterm::event as ev;
        match crossterm::event::read() {
            Err(_) => break,
            Ok(ev::Event::Resize(..)) => {}
            Ok(ev::Event::Key(event)) => match event.code {
                ev::KeyCode::Esc => {
                    break;
                }
                ev::KeyCode::Char(' ') => {
                    sim.advance();
                    // analyze the state
                    let analysis = sim.current().analyze();
                    if analysis.alive_ratio == 0.0 {
                        sim.start();
                        state.selection = None;
                        state.population_age = 0;
                    } else if analysis.alive_ratio > POPULATION_CHANGE_THRESHOLD {
                        state.population_age = 0;
                    }
                    state.population_age += 1;
                }
                _ => continue,
            },
            Ok(ev::Event::Mouse(ev::MouseEvent {
                kind: ev::MouseEventKind::Down(_),
                column,
                row,
                modifiers: _,
            })) => {
                let new = Some(grid::Coordinates {
                    x: column as _,
                    y: row as _,
                });
                state.selection = if state.selection == new { None } else { new };
            }
            Ok(ev::Event::Mouse(..)) => {
                continue;
            }
        }

        terminal.draw(|f| sim.draw(&state, f))?;
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
