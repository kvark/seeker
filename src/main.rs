mod grid;
mod lab;
mod sim;

const POPULATION_DANGER_THRESHOLD: f32 = 0.02;

#[derive(Default)]
struct SimState {
    selection: Option<grid::Coordinates>,
}

struct GridWidget<'a> {
    grid: &'a grid::Grid,
    state: &'a SimState,
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
    fn draw<B: tui::backend::Backend>(&self, state: &SimState, frame: &mut tui::Frame<B>) {
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
                            l::Constraint::Length(1),
                            l::Constraint::Length(1),
                            l::Constraint::Length(1),
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
                let occupancy_color = if analysis.alive_ratio > self.limits().min_extra_population {
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

                fn get_bits(number: usize) -> usize {
                    std::mem::size_of::<usize>() * 8 - number.leading_zeros() as usize
                }

                let age_log = get_bits(self.population().age);
                let max_age = match self.population().kind {
                    sim::PopulationKind::Intra => self.limits().max_intra_population_age,
                    sim::PopulationKind::Extra => self.limits().max_extra_population_age,
                };
                let max_age_log = get_bits(max_age);
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
                    .percent((100 * age_log / max_age_log) as u16)
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

impl lab::Laboratory {
    fn draw<B: tui::backend::Backend>(&self, frame: &mut tui::Frame<B>) {
        use tui::{
            layout as l,
            style::{Color, Style},
            text::{Span, Spans},
            widgets as w,
        };

        let experiments = self.experiments();
        let list_items = experiments
            .iter()
            .map(|experiment| {
                let mut spans = vec![
                    Span::styled(
                        format!("[{}]", experiment.id),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!(" - step {}", experiment.steps),
                        Style::default().fg(Color::DarkGray),
                    ),
                ];
                if let Some(conclusion) = experiment.conclusion {
                    spans.push(Span::raw(" ("));
                    spans.push(Span::styled(
                        format!("{:?}", conclusion),
                        Style::default().fg(Color::Blue),
                    ));
                    spans.push(Span::raw(") - "));
                    spans.push(Span::styled(
                        format!("fit {}", experiment.fit),
                        Style::default().fg(Color::Yellow),
                    ));
                }
                w::ListItem::new(vec![Spans(spans)])
            })
            .collect::<Vec<_>>();

        let experiment_list = w::List::new(list_items)
            .block(
                w::Block::default()
                    .borders(w::Borders::ALL)
                    .title("Experiments"),
            )
            .start_corner(l::Corner::TopLeft);

        let top_rects = l::Layout::default()
            .direction(l::Direction::Vertical)
            .constraints(
                [
                    l::Constraint::Length(1),
                    l::Constraint::Min(experiments.len() as u16),
                ]
                .as_ref(),
            )
            .margin(1)
            .split(frame.size());

        let progress = w::Gauge::default()
            .gauge_style(Style::default().fg(Color::White))
            .percent(self.progress_percent() as u16);
        frame.render_widget(progress, top_rects[0]);
        frame.render_widget(experiment_list, top_rects[1]);
    }
}

enum Mode {
    Play {
        sim: sim::Simulation,
        state: SimState,
    },
    Find(lab::Laboratory),
}

#[derive(Debug)]
enum ExitReason {
    Error,
    Quit,
    Done,
    Finding,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use crossterm::ExecutableCommand as _;
    use std::{fs::File, path::PathBuf};

    const MY_NAME: &str = "seeker";
    const PLAY_COMMAND: &str = "play";
    const FIND_COMMAND: &str = "find";

    let mut args = std::env::args();
    let _exec_name = args.next().unwrap();
    let command = match args.next() {
        Some(cmd) => cmd,
        None => {
            println!("Usage:");
            println!("{} {} [<path_to_snap>]", MY_NAME, PLAY_COMMAND);
            println!("{} {} <path_to_init_snap>", MY_NAME, FIND_COMMAND);
            return Ok(());
        }
    };
    let snap_name = match args.next() {
        Some(string) => string,
        None => "data/default-snap.ron".to_string(),
    };
    let mut snap_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    snap_path.push(snap_name);
    let init_snap = ron::de::from_reader(File::open(snap_path).unwrap()).unwrap();

    let mode = match command.as_str() {
        PLAY_COMMAND => Mode::Play {
            sim: sim::Simulation::new(&init_snap),
            state: SimState::default(),
        },
        FIND_COMMAND => {
            let mut config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            config_path.push("data");
            config_path.push("config.ron");
            let config = ron::de::from_reader(File::open(config_path).unwrap()).unwrap();
            let mut lab = lab::Laboratory::new(config);
            lab.add_experiment(init_snap);
            Mode::Find(lab)
        }
        _ => {
            println!("Unknown command: '{}'", command);
            return Ok(());
        }
    };

    let mut stdout = std::io::stdout();
    stdout.execute(crossterm::terminal::EnterAlternateScreen)?;
    stdout.execute(crossterm::event::EnableMouseCapture)?;
    crossterm::terminal::enable_raw_mode()?;

    let backend = tui::backend::CrosstermBackend::new(stdout);
    let mut terminal = tui::Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let reason = match mode {
        Mode::Play { mut sim, mut state } => {
            terminal.draw(|f| sim.draw(&state, f))?;
            loop {
                use crossterm::event as ev;
                match crossterm::event::read() {
                    Err(_) => break ExitReason::Error,
                    Ok(ev::Event::Resize(..)) => {}
                    Ok(ev::Event::Key(event)) => match event.code {
                        ev::KeyCode::Esc => {
                            break ExitReason::Quit;
                        }
                        ev::KeyCode::Char('s') => {
                            let snap = sim.save_snap();
                            let steps = sim.progress();
                            if let Ok(file) = File::create(format!("step-{}.ron", steps)) {
                                ron::ser::to_writer_pretty(
                                    file,
                                    &snap,
                                    ron::ser::PrettyConfig::default(),
                                )
                                .unwrap();
                            }
                        }
                        ev::KeyCode::Char(' ') => {
                            if let Err(conclusion) = sim.advance() {
                                log::info!("Conclusion: {:?}", conclusion);
                                break ExitReason::Done;
                            }
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
        }
        Mode::Find(mut lab) => {
            terminal.draw(|f| lab.draw(f))?;
            loop {
                use crossterm::event as ev;

                let event = match crossterm::event::poll(std::time::Duration::from_millis(100)) {
                    Ok(true) => crossterm::event::read(),
                    Ok(false) => Ok(ev::Event::Resize(0, 0)),
                    Err(_) => break ExitReason::Error,
                };

                match event {
                    Err(_) => break ExitReason::Error,
                    Ok(ev::Event::Resize(..)) => {}
                    Ok(ev::Event::Key(event)) => match event.code {
                        ev::KeyCode::Esc => {
                            let snap = lab.best_candidate();
                            let file = File::create("candidate.ron").unwrap();
                            ron::ser::to_writer_pretty(
                                file,
                                snap,
                                ron::ser::PrettyConfig::default(),
                            )
                            .unwrap();
                            break ExitReason::Quit;
                        }
                        _ => {}
                    },
                    Ok(ev::Event::Mouse(..)) => {
                        continue;
                    }
                }

                match lab.update() {
                    lab::LabResult::Normal => {
                        terminal.draw(|f| lab.draw(f))?;
                    }
                    lab::LabResult::Found(snap) => {
                        let file = File::create("finding.ron").unwrap();
                        ron::ser::to_writer_pretty(file, &snap, ron::ser::PrettyConfig::default())
                            .unwrap();
                        break ExitReason::Finding;
                    }
                    lab::LabResult::End => {
                        let snap = lab.best_candidate();
                        let file = File::create("candidate.ron").unwrap();
                        ron::ser::to_writer_pretty(file, &snap, ron::ser::PrettyConfig::default())
                            .unwrap();
                        break ExitReason::Done;
                    }
                }
            }
        }
    };

    crossterm::terminal::disable_raw_mode()?;
    terminal
        .backend_mut()
        .execute(crossterm::event::DisableMouseCapture)?;
    terminal
        .backend_mut()
        .execute(crossterm::terminal::LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!("{:?}", reason);
    Ok(())
}
