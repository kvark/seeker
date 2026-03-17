use seeker::{analysis, grid, lab, render, sim};

const OCCUPANCY_HISTORY: usize = 50;

#[derive(Default)]
struct WidgetState {
    selection: Option<grid::Coordinates>,
    occupancy_history: Vec<(&'static str, u64)>,
}

struct GridWidget<'a> {
    grid: &'a grid::Grid,
    state: &'a WidgetState,
}
impl ratatui::widgets::Widget for GridWidget<'_> {
    fn render(self, area: ratatui::layout::Rect, buf: &mut ratatui::buffer::Buffer) {
        use ratatui::style::Color;

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
                    ('█', color)
                } else {
                    (' ', Color::Reset)
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

                if let Some(buf_cell) = buf.cell_mut((area.x + x, area.y + y)) {
                    buf_cell.set_char(symbol).set_fg(color).set_bg(background);
                }
            }
        }
    }
}

fn draw_sim(
    sim: &sim::Simulation,
    widget_state: &WidgetState,
    frame: &mut ratatui::Frame,
) {
    use ratatui::{
        layout as l,
        style::{Color, Style},
        text::{Line, Span},
        widgets as w,
    };

    fn make_key_value<'a>(key: &'a str, value: String) -> Line<'a> {
        Line::from(vec![
            Span::styled(key, Style::default().fg(Color::DarkGray)),
            Span::raw(value),
        ])
    }

    let grid = sim.grid();
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
        .split(frame.area());

    let grid_block = w::Block::default().borders(w::Borders::ALL).title("Grid");
    let inner = grid_block.inner(top_rects[0]);
    frame.render_widget(grid_block, top_rects[0]);
    frame.render_widget(
        GridWidget {
            grid,
            state: widget_state,
        },
        inner,
    );

    {
        let meta_rects = l::Layout::default()
            .direction(l::Direction::Vertical)
            .constraints(
                [
                    l::Constraint::Min(3),
                    l::Constraint::Min(10),
                    l::Constraint::Min(5),
                ]
                .as_ref(),
            )
            .split(top_rects[1]);

        let boundary_str = match sim.grid().boundary() {
            grid::BoundaryMode::Wrap => "Wrap",
            grid::BoundaryMode::Dead => "Dead",
        };
        let para_size = w::Paragraph::new(vec![
            make_key_value("Size = ", format!("{}x{}", grid_size.x, grid_size.y)),
            make_key_value("Boundary = ", boundary_str.to_string()),
            make_key_value("Random = ", format!("{}", sim.random_seed())),
        ])
        .block(w::Block::default().title("Info").borders(w::Borders::ALL))
        .wrap(w::Wrap { trim: false });
        frame.render_widget(para_size, meta_rects[0]);

        {
            let stat_block = w::Block::default().title("Stat").borders(w::Borders::ALL);
            let stat_rects = l::Layout::default()
                .direction(l::Direction::Vertical)
                .constraints([l::Constraint::Length(1), l::Constraint::Min(5)].as_ref())
                .split(stat_block.inner(meta_rects[1]));
            frame.render_widget(stat_block, meta_rects[1]);

            let step = sim.last_step();
            let para_step = w::Paragraph::new(vec![make_key_value("Step = ", format!("{}", step))])
                .wrap(w::Wrap { trim: false });
            frame.render_widget(para_step, stat_rects[0]);

            let max_occupancy = widget_state
                .occupancy_history
                .iter()
                .map(|&(_, value)| value)
                .max()
                .unwrap_or_default();
            let occupancy_title = format!("Occupancy (max {}%)", max_occupancy / 10);
            let history_offset = widget_state
                .occupancy_history
                .len()
                .checked_sub(stat_rects[1].width as usize)
                .unwrap_or_default();
            let occupancy = w::BarChart::default()
                .block(
                    w::Block::default()
                        .borders(w::Borders::ALL)
                        .title(occupancy_title),
                )
                .data(&widget_state.occupancy_history[history_offset..])
                .bar_width(1)
                .bar_gap(0)
                .label_style(Style::default().fg(Color::Yellow))
                .bar_style(Style::default().fg(Color::Green));
            frame.render_widget(occupancy, stat_rects[1]);
        }

        if let Some(coords) = widget_state.selection {
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

fn draw_lab(lab: &lab::Laboratory, frame: &mut ratatui::Frame) {
    use ratatui::{
        layout as l,
        style::{Color, Style},
        text::{Line, Span},
        widgets as w,
    };

    let experiments = lab.experiments();
    let list_items = experiments
        .iter()
        .rev()
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
            if let Some(ref conclusion) = experiment.conclusion {
                let description = format!("{}", conclusion);
                spans.push(Span::raw(" ("));
                spans.push(Span::styled(description, Style::default().fg(Color::Blue)));
                spans.push(Span::raw(") - "));
                spans.push(Span::styled(
                    format!("fit {}", experiment.fit),
                    Style::default().fg(Color::Yellow),
                ));
            }
            w::ListItem::new(vec![Line::from(spans)])
        })
        .collect::<Vec<_>>();

    let experiment_list = w::List::new(list_items)
        .block(
            w::Block::default()
                .borders(w::Borders::ALL)
                .title("Experiments"),
        )
        .direction(w::ListDirection::TopToBottom);

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
        .split(frame.area());

    frame.render_widget(experiment_list, top_rects[1]);
}

#[allow(clippy::large_enum_variant)]
enum Mode {
    Play {
        sim: sim::Simulation,
        state: WidgetState,
    },
    Find(lab::Laboratory),
}

enum ExitReason {
    Error,
    Quit,
    Done(sim::Conclusion),
}

struct Output {
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
}

impl Output {
    fn grab() -> Result<Self, std::io::Error> {
        use crossterm::ExecutableCommand as _;

        let mut stdout = std::io::stdout();
        stdout.execute(crossterm::terminal::EnterAlternateScreen)?;
        stdout.execute(crossterm::event::EnableMouseCapture)?;
        crossterm::terminal::enable_raw_mode()?;

        let backend = ratatui::backend::CrosstermBackend::new(stdout);
        let mut terminal = ratatui::Terminal::new(backend)?;
        terminal.hide_cursor()?;
        Ok(Self { terminal })
    }

    fn release(&mut self) -> Result<(), std::io::Error> {
        use crossterm::ExecutableCommand as _;

        if std::thread::panicking() {
            // give the opportunity to see the result
            let _ = crossterm::event::read();
        }

        crossterm::terminal::disable_raw_mode()?;
        self.terminal
            .backend_mut()
            .execute(crossterm::event::DisableMouseCapture)?;
        self.terminal
            .backend_mut()
            .execute(crossterm::terminal::LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;
        Ok(())
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, fs::File, path::PathBuf};

    const MY_NAME: &str = "seeker";
    const PLAY_COMMAND: &str = "play";
    const FIND_COMMAND: &str = "find";
    const HEADLESS_COMMAND: &str = "headless";
    const REPLAY_COMMAND: &str = "replay";

    let args: Vec<String> = std::env::args().collect();
    let command = if args.len() < 2 {
        println!("Usage:");
        println!("{} {} [<path_to_snap>]", MY_NAME, PLAY_COMMAND);
        println!("{} {} [<path_to_init_snap>]", MY_NAME, FIND_COMMAND);
        println!(
            "{} {} [<path_to_init_snap>] [<duration_secs>] [<config_path>]",
            MY_NAME, HEADLESS_COMMAND
        );
        println!("{} {} <path_to_snap> <output.gif>", MY_NAME, REPLAY_COMMAND);
        return Ok(());
    } else {
        args[1].clone()
    };

    let snap_name = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "data/default-snap.ron".to_string());
    let mut snap_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    snap_path.push(&snap_name);
    let init_snap: sim::Snap =
        ron::de::from_reader(File::open(&snap_path).unwrap()).unwrap();

    let mode = match command.as_str() {
        PLAY_COMMAND => Mode::Play {
            sim: sim::Simulation::new(&init_snap).unwrap(),
            state: WidgetState::default(),
        },
        FIND_COMMAND => {
            let mut config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            config_path.push("data");
            config_path.push("config.ron");
            let config = ron::de::from_reader(File::open(config_path).unwrap()).unwrap();
            let mut lab = lab::Laboratory::new(config, "data/active/");
            lab.add_experiment(init_snap, 0);
            Mode::Find(lab)
        }
        HEADLESS_COMMAND => {
            let duration_secs: u64 = args
                .get(3)
                .and_then(|s| s.parse().ok())
                .unwrap_or(120);
            let config_name = args
                .get(4)
                .cloned()
                .unwrap_or_else(|| "data/config.ron".to_string());
            let mut config_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            config_path.push(&config_name);
            let config = ron::de::from_reader(File::open(config_path).unwrap()).unwrap();
            let mut lab = lab::Laboratory::new(config, "data/active/");
            lab.add_experiment(init_snap, 0);
            eprintln!("Running headless search for {}s...", duration_secs);
            let start = std::time::Instant::now();
            let deadline = std::time::Duration::from_secs(duration_secs);
            while start.elapsed() < deadline {
                lab.update();
                std::thread::sleep(std::time::Duration::from_millis(10));
                let experiments = lab.experiments();
                let concluded = experiments.iter().filter(|e| e.conclusion.is_some()).count();
                let active = experiments.len() - concluded;
                let max_fit = experiments.iter().map(|e| e.fit).max().unwrap_or(0);
                eprint!(
                    "\r[{} total, {} active, {} concluded, {} discarded] best fit: {}    ",
                    experiments.len(),
                    active,
                    concluded,
                    lab.early_discards,
                    max_fit
                );
            }
            eprintln!();

            // Collect interesting concluded experiments (survivors only)
            let mut survivors: Vec<_> = lab
                .experiments()
                .iter()
                .filter(|e| {
                    matches!(
                        e.conclusion,
                        Some(sim::Conclusion::Done(..))
                    )
                })
                .collect();
            survivors.sort_by(|a, b| b.fit.cmp(&a.fit));

            // Print structured summary
            let total = lab.experiments().len();
            let extinct_count = lab
                .experiments()
                .iter()
                .filter(|e| matches!(e.conclusion, Some(sim::Conclusion::Extinct)))
                .count();
            let saturate_count = lab
                .experiments()
                .iter()
                .filter(|e| matches!(e.conclusion, Some(sim::Conclusion::Saturate)))
                .count();

            println!("## Search Results");
            println!();
            println!("- Duration: {}s", duration_secs);
            println!("- Total experiments: {}", total);
            println!("- Survivors: {}", survivors.len());
            println!("- Extinct: {}", extinct_count);
            println!("- Saturated: {}", saturate_count);
            println!();

            if !survivors.is_empty() {
                println!("### Top Survivors");
                println!();
                println!("| ID | Fitness | Alive Avg | Alive Var | Birth Rate | Spatial Var | Period | Steps | Snap |");
                println!("|----|---------|-----------|-----------|------------|-------------|--------|-------|------|");
                let top_n = survivors.len().min(10);
                for exp in &survivors[..top_n] {
                    if let Some(sim::Conclusion::Done(stats, _)) = &exp.conclusion {
                        let snap_file = format!("e{}-{}.ron", exp.id, exp.steps);
                        println!(
                            "| {} | {} | {:.4} | {:.6} | {:.6} | {:.6} | {} | {} | {} |",
                            exp.id, exp.fit, stats.alive_ratio_average,
                            stats.alive_ratio_variance, stats.birth_rate_average,
                            stats.spatial_variance_average,
                            stats.period, exp.steps, snap_file
                        );
                    }
                }
                println!();

                // Pattern analysis for top survivors
                println!("### Pattern Analysis");
                println!();
                let analyze_count = top_n.min(10);
                for exp in &survivors[..analyze_count] {
                    if let Some(sim::Conclusion::Done(..)) = &exp.conclusion {
                        // Re-run simulation to get the stabilized grid
                        if let Ok(mut sim) = sim::Simulation::new(exp.snap()) {
                            loop {
                                match sim.advance() {
                                    Ok(_) => {}
                                    Err(_) => break,
                                }
                            }
                            let (_patterns, summary) = analysis::analyze_grid(sim.grid());
                            print!("- E[{}]: {}", exp.id, summary);
                            // Highlight interesting finds
                            if !summary.spaceships.is_empty() {
                                print!(" **SPACESHIP FOUND!**");
                            }
                            let high_period: Vec<_> = summary
                                .oscillators
                                .iter()
                                .filter(|&&p| p > 2)
                                .collect();
                            if !high_period.is_empty() {
                                print!(
                                    " **HIGH-PERIOD OSC: {:?}**",
                                    high_period
                                );
                            }
                            println!();
                        }
                    }
                }
                println!();

                // Record GIFs for interesting survivors
                let gif_count = top_n.min(5);
                let gif_dir = PathBuf::from("data/active/gifs");
                fs::create_dir_all(&gif_dir).unwrap();
                println!("### Recorded GIFs");
                println!();
                for exp in &survivors[..gif_count] {
                    let gif_path = gif_dir.join(format!("e{}.gif", exp.id));
                    eprint!("Recording GIF for E[{}]...", exp.id);
                    let mut sim = sim::Simulation::new(exp.snap()).unwrap();
                    match render::record_gif(&mut sim, &gif_path, 4, 200) {
                        Ok(frames) => {
                            eprintln!(" {} frames", frames);
                            println!("- `gifs/e{}.gif` ({} frames)", exp.id, frames);
                        }
                        Err(e) => {
                            eprintln!(" error: {}", e);
                        }
                    }
                }
            }

            return Ok(());
        }
        REPLAY_COMMAND => {
            let output_path = args
                .get(3)
                .map(|s| PathBuf::from(s))
                .unwrap_or_else(|| PathBuf::from("replay.gif"));
            let mut sim = sim::Simulation::new(&init_snap).unwrap();
            eprintln!(
                "Replaying {} -> {}",
                snap_path.display(),
                output_path.display()
            );
            match render::record_gif(&mut sim, &output_path, 4, 200) {
                Ok(frames) => {
                    eprintln!("Wrote {} frames to {}", frames, output_path.display());
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        _ => {
            println!("Unknown command: '{}'", command);
            return Ok(());
        }
    };

    let reason = {
        use crossterm::event as ev;

        let mut output = Output::grab()?;
        match mode {
            Mode::Play { mut sim, mut state } => {
                output.terminal.draw(|f| draw_sim(&sim, &state, f))?;
                loop {
                    match ev::read() {
                        Err(_) => break ExitReason::Error,
                        Ok(ev::Event::Resize(..)) => {}
                        Ok(ev::Event::Key(event)) => match event.code {
                            ev::KeyCode::Esc => {
                                break ExitReason::Quit;
                            }
                            ev::KeyCode::Char('s') => {
                                let snap = sim.save_snap();
                                let steps = sim.last_step();
                                if let Ok(file) = File::create(format!("step-{}.ron", steps)) {
                                    ron::ser::to_writer_pretty(
                                        file,
                                        &snap,
                                        ron::ser::PrettyConfig::default(),
                                    )
                                    .unwrap();
                                }
                            }
                            ev::KeyCode::Char(' ') => match sim.advance() {
                                Ok(analysis) => {
                                    if state.occupancy_history.len() >= OCCUPANCY_HISTORY {
                                        state.occupancy_history.remove(0);
                                    }
                                    state
                                        .occupancy_history
                                        .push(("", (analysis.alive_ratio * 1000.0) as u64));
                                }
                                Err(conclusion) => {
                                    break ExitReason::Done(conclusion);
                                }
                            },
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
                        Ok(ev::Event::Mouse(..))
                        | Ok(ev::Event::FocusGained)
                        | Ok(ev::Event::FocusLost)
                        | Ok(ev::Event::Paste(_)) => {
                            continue;
                        }
                    }

                    output.terminal.draw(|f| draw_sim(&sim, &state, f))?;
                }
            }
            Mode::Find(mut lab) => {
                output.terminal.draw(|f| draw_lab(&lab, f))?;
                loop {
                    let event = match ev::poll(std::time::Duration::from_millis(100)) {
                        Ok(true) => ev::read(),
                        Ok(false) => Ok(ev::Event::Resize(0, 0)),
                        Err(_) => break ExitReason::Error,
                    };

                    match event {
                        Err(_) => break ExitReason::Error,
                        Ok(ev::Event::Resize(..)) => {}
                        #[allow(clippy::single_match)]
                        Ok(ev::Event::Key(event)) => match event.code {
                            ev::KeyCode::Esc => break ExitReason::Quit,
                            _ => {}
                        },
                        Ok(ev::Event::Mouse(..))
                        | Ok(ev::Event::FocusGained)
                        | Ok(ev::Event::FocusLost)
                        | Ok(ev::Event::Paste(_)) => {
                            continue;
                        }
                    }

                    lab.update();
                    output.terminal.draw(|f| draw_lab(&lab, f))?;
                }
            }
        }
    };

    if let ExitReason::Done(conclusion) = reason {
        println!("{}", conclusion);
    }
    Ok(())
}
